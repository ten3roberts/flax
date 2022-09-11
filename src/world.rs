use core::fmt;
use std::{
    collections::BTreeMap,
    fmt::Formatter,
    mem::{self, MaybeUninit},
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use itertools::Itertools;

use crate::{
    archetype::{Archetype, ArchetypeId, BatchSpawn, Change, ComponentInfo, Slice},
    buffer::ComponentBuffer,
    components::{is_component, name},
    debug_visitor,
    entity::*,
    entity_ref::{EntityRef, EntityRefMut},
    entry::{Entry, OccupiedEntry, VacantEntry},
    error::Result,
    is_static_component, Component, ComponentId, ComponentValue, Entity, Error, Filter, Query,
    RelationExt, RowFormatter, StaticFilter,
};

#[derive(Debug, Default)]
struct EntityStores {
    inner: BTreeMap<EntityKind, EntityStore>,
}

impl EntityStores {
    fn init(&mut self, kind: EntityKind) -> &mut EntityStore {
        self.inner
            .entry(kind)
            .or_insert_with(|| EntityStore::new(kind))
    }

    fn get(&self, kind: EntityKind) -> Option<&EntityStore> {
        self.inner.get(&kind)
    }
}

pub(crate) struct Archetypes {
    root: ArchetypeId,
    gen: u32,
    inner: EntityStore<Archetype>,
}

impl Archetypes {
    pub fn new() -> Self {
        let mut archetypes = EntityStore::new(EntityKind::empty());
        let root = archetypes.spawn(Archetype::empty());
        Self {
            root,
            inner: archetypes,
            gen: 0,
        }
    }

    pub fn get(&self, arch_id: ArchetypeId) -> &Archetype {
        self.inner.get(arch_id).expect("Invalid archetype")
    }

    pub fn get_mut(&mut self, arch_id: ArchetypeId) -> &mut Archetype {
        let arch = self.inner.get_mut(arch_id).expect("Invalid archetype");

        arch
    }

    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    fn init<I: std::borrow::Borrow<ComponentInfo>>(
        &mut self,
        components: impl IntoIterator<Item = I>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut cursor = self.root;

        for head in components {
            let head = head.borrow();
            let cur = &mut self.inner.get(cursor).expect("Invalid archetype id");

            cursor = match cur.edge_to(head.id) {
                Some(id) => id,
                None => {
                    let new = Archetype::new(cur.components().copied().chain([*head]));

                    // Increase gen
                    self.gen = self.gen.wrapping_add(1);
                    let id = self.inner.spawn(new);

                    let (cur, new) = self.inner.get_disjoint(cursor, id).unwrap();
                    cur.add_edge_to(new, id, cursor, head.id);

                    id
                }
            };
        }

        (cursor, self.inner.get_mut(cursor).unwrap())
    }

    pub fn root(&mut self) -> &mut Archetype {
        self.get_mut(self.root)
    }

    pub fn get_disjoint(
        &mut self,
        a: Entity,
        b: Entity,
    ) -> Option<(&mut Archetype, &mut Archetype)> {
        let (a, b) = self.inner.get_disjoint(a, b)?;

        Some((a, b))
    }

    pub fn iter(&self) -> EntityStoreIter<Archetype> {
        self.inner.iter()
    }

    /// Despawn an archetype, leaving a hole in the tree.
    ///
    /// It is the callers responibility to cleanup child nodes if the node is internal
    /// Children are detached from the tree, but still accesible by id
    fn despawn(&mut self, id: Entity) -> Archetype {
        let arch = self.inner.despawn(id).expect("Invalid archetype");

        // Remove outgoing edges
        for (component, dst_id) in &arch.edges {
            assert!(self.get_mut(*dst_id).edges.remove(component).is_some());
        }
        self.gen = self.gen.wrapping_add(1);

        arch
    }
}

type EventSender = Box<dyn Fn(Entity, *const u8) -> bool + Send + Sync>;

#[derive(Default)]
struct EventRegistry {
    inner: BTreeMap<ComponentId, Vec<EventSender>>,
}

impl EventRegistry {
    fn register(&mut self, component: ComponentId, sender: EventSender) {
        self.inner.entry(component).or_default().push(sender)
    }

    fn send(&mut self, component: ComponentId, id: Entity, value: *const u8) {
        if let Some(senders) = self.inner.get_mut(&component) {
            senders.retain_mut(|v| v(id, value));
        }
    }
}

/// Holds the entities and components of the ECS.
pub struct World {
    entities: EntityStores,
    pub(crate) archetypes: Archetypes,
    change_tick: AtomicU32,

    on_removed: EventRegistry,
}

impl World {
    /// Creates a new empty world
    pub fn new() -> Self {
        Self {
            entities: EntityStores::default(),
            archetypes: Archetypes::new(),
            change_tick: AtomicU32::new(0b11),
            on_removed: EventRegistry::default(),
        }
    }

    /// Create an iterator to spawn several entities
    pub fn spawn_many(&mut self) -> impl Iterator<Item = Entity> + '_ {
        (0..).map(|_| self.spawn())
    }

    /// Spawn a new empty entity into the default namespace
    pub fn spawn(&mut self) -> Entity {
        self.spawn_inner(self.archetypes.root, EntityKind::empty())
            .0
    }

    /// Efficiently spawn many entities with the same components at once.
    pub fn spawn_batch(&mut self, batch: &mut BatchSpawn) -> Vec<Entity> {
        for &component in batch.components() {
            self.init_component(component)
                .expect("Failed to initialize component");
        }

        let change_tick = self.advance_change_tick();

        let (arch_id, arch) = self.archetypes.init(batch.components());

        let base = arch.len();
        let store = self.entities.init(EntityKind::empty());

        let ids = (0..batch.len())
            .map(|idx| {
                store.spawn(EntityLocation {
                    slot: base + idx,
                    arch: arch_id,
                })
            })
            .collect_vec();

        let slots = arch.allocate_n(&ids);

        for (_, mut storage) in batch.take_all() {
            unsafe {
                arch.extend(&mut storage)
                    .expect("Component not in archetype");
            }

            arch.init_changes(*storage.info())
                .set_inserted(Change::new(slots, change_tick));
        }

        ids
    }

    /// Batch spawn multiple components with prespecified ids.
    pub fn spawn_batch_at(&mut self, ids: &[Entity], batch: &mut BatchSpawn) -> Result<()> {
        assert_eq!(
            ids.len(),
            batch.len(),
            "The length of ids must match the number of slots in `batch`"
        );

        for &component in batch.components() {
            self.init_component(component)
                .expect("failed to initialize component");
        }

        for &id in ids {
            assert!(matches!(
                self.despawn(id),
                Err(Error::NoSuchEntity(..)) | Ok(())
            ));
        }

        let change_tick = self.advance_change_tick();

        let (arch_id, arch) = self.archetypes.init(batch.components());

        let base = arch.len();
        for (idx, &id) in ids.iter().enumerate() {
            let kind = id.kind();
            let store = self.entities.init(kind);
            assert_eq!(store.kind, kind);
            store
                .spawn_at(
                    id.index(),
                    id.generation(),
                    EntityLocation {
                        slot: base + idx,
                        arch: arch_id,
                    },
                )
                .unwrap_or_else(|| panic!("Entity {id} already exists"));
        }

        let slots = arch.allocate_n(ids);

        let arch = self.archetypes.get_mut(arch_id);

        for (_, mut storage) in batch.take_all() {
            unsafe {
                arch.extend(&mut storage)
                    .expect("Component not in archetype");
            }

            arch.init_changes(*storage.info())
                .set_inserted(Change::new(slots, change_tick));
        }

        Ok(())
    }

    /// Spawn a new component of type `T` which can be attached to an entity.
    ///
    /// The given name does not need to be unique.
    pub fn spawn_component<T: ComponentValue>(
        &mut self,
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
    ) -> Component<T> {
        let (id, _, _) = self.spawn_inner(self.archetypes.root, EntityKind::COMPONENT);

        // Safety
        // The id is not used by anything else
        let component = unsafe { Component::from_raw_id(id, name, meta) };

        self.init_component(component.info()).unwrap();
        component
    }

    /// Spawn a new relation of type `T` which can be attached to an entity.
    ///
    /// The given name does not need to be unique.
    pub fn spawn_relation<T: ComponentValue>(
        &mut self,
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
    ) -> impl Fn(Entity) -> Component<T> {
        let (id, _, _) = self.spawn_inner(
            self.archetypes.root,
            EntityKind::COMPONENT | EntityKind::RELATION,
        );

        let relation = move |object| unsafe { Component::from_pair(id, name, meta, object) };
        self.init_component(relation(wildcard()).info()).unwrap();

        relation
    }

    fn spawn_inner(
        &mut self,
        arch_id: ArchetypeId,
        kind: EntityKind,
    ) -> (Entity, EntityLocation, &mut Archetype) {
        // Place at root
        let ns = self.entities.init(kind);

        let arch = self.archetypes.get_mut(arch_id);

        let slot = arch.len();

        let loc = EntityLocation {
            arch: arch_id,
            slot,
        };

        let id = ns.spawn(loc);

        arch.allocate(id);

        (id, loc, arch)
    }

    /// Spawns an entitiy with a specific id.
    /// Despawns any existing entity.
    pub fn spawn_at(&mut self, id: Entity) {
        self.spawn_at_inner(id, self.archetypes.root);
    }

    /// Spawns an entitiy with a specific id.
    /// Despawns any existing entity.
    fn spawn_at_inner(
        &mut self,
        id: Entity,
        arch_id: ArchetypeId,
    ) -> (EntityLocation, &mut Archetype) {
        assert!(matches!(
            self.despawn(id),
            Err(Error::NoSuchEntity(..)) | Ok(())
        ));

        let store = self.entities.init(id.kind());
        let arch = self.archetypes.get_mut(arch_id);

        let loc = store
            .spawn_at(
                id.index(),
                id.generation(),
                EntityLocation {
                    slot: 0,
                    arch: arch_id,
                },
            )
            .expect("Entity not despawned");

        loc.slot = arch.allocate(id);

        (*loc, arch)
    }

    /// Spawn an entity with the given components.
    ///
    /// For increased ergonomics, prefer [crate::EntityBuilder]
    pub fn spawn_at_with(&mut self, id: Entity, buffer: &mut ComponentBuffer) -> Entity {
        let change_tick = self.advance_change_tick();

        let (arch_id, _) = self.archetypes.init(buffer.components());
        let (loc, arch) = self.spawn_at_inner(id, arch_id);

        for (component, src) in buffer.take_all() {
            unsafe {
                arch.push(component.id, src)
                    .expect("Component not in archetype")
            }

            arch.init_changes(component)
                .set_inserted(Change::new(Slice::single(loc.slot), change_tick));
        }

        for &component in buffer.components() {
            self.init_component(component)
                .expect("Failed to initialize component");
        }

        id
    }

    /// Spawn an entity with the given components.
    ///
    /// For increased ergonomics, prefer [crate::EntityBuilder]
    pub fn spawn_with(&mut self, buffer: &mut ComponentBuffer) -> Entity {
        let change_tick = self.advance_change_tick();

        for component in buffer.components() {
            self.init_component(*component)
                .expect("Failed to initialize component");
        }

        // assert_eq!(
        //     buffer.components().sorted().collect_vec(),
        //     buffer.components().collect_vec(),
        //     "Components are not sorted"
        // );

        let (arch_id, _) = self.archetypes.init(buffer.components());

        let (id, loc, arch) = self.spawn_inner(arch_id, EntityKind::empty());

        for (component, src) in buffer.take_all() {
            unsafe {
                arch.push(component.id, src)
                    .expect("Component not in archetype")
            }

            arch.init_changes(component)
                .set_inserted(Change::new(Slice::single(loc.slot), change_tick));
        }

        id
    }

    /// Removes all components from an entity without despawning the entity
    pub fn clear(&mut self, id: Entity) -> Result<()> {
        let EntityLocation { arch, slot } = self.location(id)?;

        let src = self.archetypes.get_mut(arch);

        let swapped = unsafe {
            src.take(slot, |c, p| {
                self.on_removed.send(c.id(), id, p);
                (c.drop)(p)
            })
        };

        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            self.entities
                .init(swapped.kind())
                .get_mut(swapped)
                .expect("Invalid entity id")
                .slot = slot;
        }

        *self.location_mut(id).unwrap() = EntityLocation {
            slot: self.archetypes.root().allocate(id),
            arch: self.archetypes.root,
        };

        self.detach(id);
        Ok(())
    }

    /// Add the components stored in a component buffer to an entity
    pub fn set_with(&mut self, id: Entity, components: &mut ComponentBuffer) -> Result<()> {
        let id: Entity = id;
        let change_tick = self.advance_change_tick();

        let EntityLocation { arch, slot } = self.location(id)?;

        let mut new_data = Vec::new();
        let mut new_components = Vec::new();

        let src_id = arch;

        for (component, data) in components.take_all() {
            let src = self.archetypes.get_mut(arch);

            if let Some(old) = src.get_dyn(slot, component.id) {
                // Drop old and copy the new value in
                unsafe {
                    (component.drop)(old);
                    ptr::copy_nonoverlapping(data, old, component.size());
                }

                src.changes_mut(component.id())
                    .unwrap()
                    .set_modified_if_tracking(Change::new(Slice::single(slot), change_tick));
            } else {
                // Component does not exist yet, so defer a move

                // Data will have a lifetime of `components`.
                new_data.push((component, data));
                new_components.push(component);
            }
        }

        if !new_data.is_empty() {
            debug_assert_eq!(new_data.len(), new_components.len());
            let src = self.archetypes.get_mut(arch);
            new_components.extend(src.components().copied());

            // Make sure everything is in its order
            #[cfg(feature = "internal_assert")]
            {
                let v = new_components.iter().sorted().cloned().collect_vec();
                assert_eq!(new_components, v, "set_with not in order");
            }

            let components = new_components;

            let (dst_id, _) = self.archetypes.init(components.iter());

            // Borrow disjoint
            let (src, dst) = self.archetypes.get_disjoint(src_id, dst_id).unwrap();

            // dst.push is called immediately
            unsafe {
                let (dst_slot, swapped) =
                    src.move_to(dst, slot, |c, _| panic!("Component {c:#?} was removed"));

                // Insert the missing components
                for &(component, data) in &new_data {
                    dst.push(component.id, data)
                        .expect("Insert should not fail");

                    dst.init_changes(component)
                        .set_inserted(Change::new(Slice::single(dst_slot), change_tick));
                }

                assert_eq!(dst.entity(dst_slot), Some(id));

                if let Some((swapped, slot)) = swapped {
                    // The last entity in src was moved into the slot occupied by id
                    self.entities
                        .init(swapped.kind())
                        .get_mut(swapped)
                        .expect("Invalid entity id")
                        .slot = slot;
                }

                *self.location_mut(id).expect("Entity is not valid") = EntityLocation {
                    slot: dst_slot,
                    arch: dst_id,
                };
            }
        }

        for (component, _) in new_data {
            if component.id != id {
                self.init_component(component)
                    .expect("Failed to initialize component");
            }
        }

        Ok(())
    }

    /// Set metadata for a given component if they do not already exist
    fn init_component(&mut self, info: ComponentInfo) -> Result<ComponentInfo> {
        assert!(
            info.id().kind().contains(EntityKind::COMPONENT),
            "Component is not a component kind id"
        );

        if self.has(info.id(), is_component()) {
            return Ok(info);
        }

        let mut meta = info.meta()(info);
        meta.set(is_component(), info);
        meta.set(name(), info.name().to_string());

        if info.id().is_static() {
            meta.set(is_static_component(), ());
        }

        self.spawn_at(info.id());
        self.set_with(info.id(), &mut meta).unwrap();

        Ok(info)
    }

    /// Despawn an entity.
    /// Any relations to other entities will be removed.
    pub fn despawn(&mut self, id: Entity) -> Result<()> {
        let EntityLocation { arch, slot } = self.location(id)?;

        if id.is_static() {
            panic!("Attempt to despawn static component");
        }

        let src = self.archetypes.get_mut(arch);

        let swapped = unsafe {
            src.take(slot, |c, p| {
                self.on_removed.send(c.id(), id, p);
                (c.drop)(p);
            })
        };

        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            self.entities
                .init(swapped.kind())
                .get_mut(swapped)
                .expect("Invalid entity id")
                .slot = slot;
        }

        self.entities.init(id.kind()).despawn(id)?;
        self.detach(id);
        Ok(())
    }

    /// Despawns all entities which matches the filter
    pub fn despawn_many<F>(&mut self, filter: F)
    where
        F: for<'x> Filter<'x>,
    {
        let mut query = Query::new(entity_ids()).filter(filter);
        let ids = query.borrow(self).iter().collect_vec();

        for id in ids {
            self.despawn(id).expect("Invalid entity id");
        }
    }

    /// Despawns an entity and all connected entities through the supplied
    /// relation
    pub fn despawn_recursive<T: ComponentValue>(
        &mut self,
        id: Entity,
        relation: impl RelationExt<T>,
    ) -> Result<()> {
        let mut to_remove = vec![id];

        while let Some(id) = to_remove.pop() {
            for (_, arch) in self
                .archetypes
                .iter()
                .filter(|(_, arch)| arch.relations().any(|v| v == relation.of(id).id()))
            {
                to_remove.extend_from_slice(arch.entities());
            }

            self.despawn(id)?;
        }

        Ok(())
    }

    /// Removes all instances of relations and component of the given entities
    /// in the world. If used upon an entity with a child -> parent relation, this removes the relation
    /// on all the children.
    pub fn detach(&mut self, id: ComponentId) {
        // The archetypes to remove
        let archetypes = self
            .archetypes()
            .filter(|(_, v)| {
                let remove = v.components().any(|v| {
                    v.id() == id
                        || (id.is_relation() && v.id().low() == id.low())
                        || (!id.is_relation() && v.id().high() == id.low())
                });

                remove
            })
            .map(|v| v.0)
            .collect_vec();

        for src in archetypes {
            let mut src = self.archetypes.despawn(src);

            let components = src.components().filter(|v| {
                !(v.id() == id
                    || (id.is_relation() && v.id().low() == id.low())
                    || (!id.is_relation() && v.id().high() == id.low()))
            });

            let (dst_id, dst) = self.archetypes.init(components);

            for (id, slot) in src.move_all(dst) {
                *self.location_mut(id).expect("Entity id was not valid") =
                    EntityLocation { slot, arch: dst_id }
            }
        }
    }

    /// Set the value of a component.
    /// If the component does not exist it will be added.
    pub fn set<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> Result<Option<T>> {
        self.set_inner(id, component, value).map(|v| v.0)
    }

    pub(crate) fn set_dyn(
        &mut self,
        id: Entity,
        info: ComponentInfo,
        value: *mut u8,
        on_drop: impl FnOnce(*mut u8),
    ) -> Result<EntityLocation> {
        // We know things will change either way
        let change_tick = self.advance_change_tick();

        let EntityLocation { arch: src_id, slot } = self.location(id)?;

        let src = self.archetypes.get_mut(src_id);

        if let Some(old) = src.get_dyn(slot, info.id()) {
            src.changes_mut(info.id())
                .expect("Missing change list")
                .set_modified_if_tracking(Change::new(Slice::single(slot), change_tick));

            // Make the caller responsible for drop or store
            (on_drop(old));

            unsafe {
                ptr::copy_nonoverlapping(value, old, info.size());
            }

            return Ok(EntityLocation { arch: src_id, slot });
        }

        // Pick up the entity and move it to the destination archetype
        let dst_id = match src.edge_to(info.id()) {
            Some(dst) => dst,
            None => {
                let pivot = src.components().take_while(|v| v.id < info.id()).count();

                // Split the components
                // A B C [new] D E F
                let left = src.components().take(pivot).copied();
                let right = src.components().skip(pivot).copied();

                let components: Vec<_> = left.chain([info]).chain(right).collect();

                // assert in order
                let (dst_id, _) = self.archetypes.init(&components);

                dst_id
            }
        };

        unsafe {
            assert_ne!(src_id, dst_id);
            // Borrow disjoint
            let (src, dst) = self.archetypes.get_disjoint(src_id, dst_id).unwrap();

            let (dst_slot, swapped) =
                src.move_to(dst, slot, |c, _| panic!("Component {c:#?} was removed"));

            // Add a quick edge to refer to later
            src.add_edge_to(dst, dst_id, src_id, info.id());

            // Insert the missing component
            dst.push(info.id, value).expect("Insert should not fail");

            debug_assert_eq!(dst.entity(dst_slot), Some(id));

            dst.init_changes(info)
                .set_inserted(Change::new(Slice::single(dst_slot), change_tick));

            if let Some((swapped, slot)) = swapped {
                // The last entity in src was moved into the slot occupied by id
                let swapped_ns = self.entities.init(swapped.kind());
                swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
            }

            let ns = self.entities.init(id.kind());
            let loc = EntityLocation {
                slot: dst_slot,
                arch: dst_id,
            };
            *ns.get_mut(id).expect("Entity is not valid") = loc;

            if info.id() != id {
                self.init_component(info)
                    .expect("Failed to initialize component");
            }

            Ok(loc)
        }
    }

    #[inline]
    pub(crate) fn set_inner<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        mut value: T,
    ) -> Result<(Option<T>, EntityLocation)> {
        let mut old: Option<T> = None;

        let loc = self.set_dyn(
            id,
            component.info(),
            &mut value as *mut T as *mut u8,
            |ptr| unsafe { old = Some(ptr.cast::<T>().read()) },
        )?;

        mem::forget(value);

        Ok((old, loc))
    }

    pub(crate) fn remove_dyn(&mut self, id: Entity, component: ComponentInfo) -> Result<()> {
        unsafe {
            self.remove_inner(id, component, |ptr| (component.drop)(ptr))
                .map(|_| {})
        }
    }

    pub(crate) unsafe fn remove_inner(
        &mut self,
        id: Entity,
        component: ComponentInfo,
        on_drop: impl FnOnce(*mut u8),
    ) -> Result<EntityLocation> {
        let ns = self.entities.init(id.kind());
        let &EntityLocation { arch: src_id, slot } = ns.get(id).unwrap();

        let src = self.archetypes.get(src_id);

        if !src.has(component.id()) {
            return Err(Error::MissingComponent(id, component.name));
        }

        let dst_id = match src.edge_to(component.id()) {
            Some(dst) => dst,
            None => {
                let components: Vec<_> = src
                    .components()
                    .filter(|v| v.id != component.id())
                    .copied()
                    .collect_vec();

                let (dst_id, _) = self.archetypes.init(&components);

                dst_id
            }
        };

        let change_tick = self.advance_change_tick();

        assert_ne!(src_id, dst_id);
        // Borrow disjoint
        let (src, dst) = self.archetypes.get_disjoint(src_id, dst_id).unwrap();

        // Add a quick edge to refer to later
        src.add_edge_to(dst, dst_id, src_id, component.id());

        // Take the value
        // This moves the differing value out of the archetype before it is
        // forgotten in the move

        // Capture the ONE moved value
        let mut on_drop = Some(on_drop);
        let (dst_slot, swapped) = src.move_to(dst, slot, |_, p| {
            let drop = on_drop.take().expect("On drop called more than once");
            self.on_removed.send(component.id(), id, p);
            (drop)(p);
        });

        assert_eq!(dst.entity(dst_slot), Some(id));

        // Migrate all changes
        dst.init_changes(component)
            .set_removed(Change::new(Slice::single(dst_slot), change_tick));

        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            let swapped_ns = self.entities.init(swapped.kind());
            swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        let loc = EntityLocation {
            slot: dst_slot,
            arch: dst_id,
        };

        *self.location_mut(id).expect("Entity is not valid") = loc;

        Ok(loc)
    }

    /// Remove a a component from the entity
    pub fn remove<T: ComponentValue>(&mut self, id: Entity, component: Component<T>) -> Result<T> {
        let mut res: MaybeUninit<T> = MaybeUninit::uninit();
        let res = unsafe {
            self.remove_inner(id, component.info(), |ptr| {
                res.write(ptr.cast::<T>().read());
            })?;
            res.assume_init()
        };
        Ok(res)
    }

    /// Randomly access an entity's component.
    pub fn get<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Result<AtomicRef<T>> {
        let loc = self.location(id)?;

        self.archetypes
            .get(loc.arch)
            .get(loc.slot, component)
            .ok_or_else(|| Error::MissingComponent(id, component.name()))
    }

    pub(crate) fn get_at<T: ComponentValue>(
        &self,
        EntityLocation { arch, slot }: EntityLocation,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        self.archetypes.get(arch).get(slot, component)
    }
    /// Randomly access an entity's component.
    pub fn get_mut<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Result<AtomicRefMut<T>> {
        let loc = self.location(id)?;

        self.get_mut_at(loc, component)
            .ok_or_else(|| Error::MissingComponent(id, component.name()))
    }
    /// Randomly access an entity's component.
    pub(crate) fn get_mut_at<T: ComponentValue>(
        &self,
        EntityLocation { arch, slot }: EntityLocation,
        component: Component<T>,
    ) -> Option<AtomicRefMut<T>> {
        let archetype = self.archetypes.get(arch);

        let change_tick = self.advance_change_tick();

        if !archetype.has(component.id()) {
            return None;
        }

        archetype
            .changes_mut(component.id())
            .expect("Change list is empty")
            .set_modified_if_tracking(Change::new(Slice::single(slot), change_tick));

        archetype.get_mut(slot, component)
    }

    /// Returns true if the entity has the specified component.
    /// Returns false if the entity does not exist or it does not have the
    /// specified component
    pub fn has<T: ComponentValue>(&self, id: Entity, component: Component<T>) -> bool {
        if let Ok(loc) = self.location(id) {
            self.archetypes.get(loc.arch).has(component.id())
        } else {
            false
        }
    }

    /// Returns true if the entity is still alive
    pub fn is_alive(&self, id: Entity) -> bool {
        self.entities
            .get(id.kind())
            .map(|v| v.is_alive(id))
            .unwrap_or(false)
    }

    pub(crate) fn archetypes(&self) -> impl Iterator<Item = (ArchetypeId, &Archetype)> {
        self.archetypes.iter()
    }

    /// Returns the location inside an archetype for a given entity
    pub(crate) fn location(&self, id: Entity) -> Result<EntityLocation> {
        self.entities
            .get(id.kind())
            .and_then(|v| v.get(id))
            .ok_or(Error::NoSuchEntity(id))
            .copied()
    }

    fn location_mut(&mut self, id: Entity) -> Result<&mut EntityLocation> {
        self.entities
            .init(id.kind())
            .get_mut(id)
            .ok_or(Error::NoSuchEntity(id))
    }

    /// Get a reference to the world's archetype generation
    #[must_use]
    pub fn archetype_gen(&self) -> u32 {
        self.archetypes.gen
    }

    #[must_use]
    /// Returns the current world change tick
    pub fn change_tick(&self) -> u32 {
        (self.change_tick.fetch_or(1, Ordering::Relaxed) >> 1) + 1
    }

    /// Increases the change tick and returns the new one
    pub(crate) fn advance_change_tick(&self) -> u32 {
        let v = self
            .change_tick
            .fetch_update(Ordering::Acquire, Ordering::Relaxed, |v| {
                // No read bit
                // No need to update
                if v & 1 == 0 {
                    None
                } else {
                    Some(v + 1)
                    // v is not even and not read
                }
            });

        match v {
            Ok(v) => ((v + 1) >> 1) + 1,
            Err(v) => (v >> 1) + 1,
        }
    }

    /// Formats the world using the debug visitor.
    pub fn format_debug<F>(&self, filter: F) -> WorldFormatter<F>
    where
        F: StaticFilter,
    {
        WorldFormatter {
            world: self,
            filter,
        }
    }

    /// Formats a set of entities using the debug visitor.
    pub fn format_entities<'a>(&'a self, ids: &'a [Entity]) -> EntityFormatter<'a> {
        EntityFormatter { world: self, ids }
    }

    /// Attempt to find an alive entity given the id
    pub fn find_alive(&self, id: StrippedEntity) -> Option<Entity> {
        let ns = self.entities.get(id.kind())?;

        ns.reconstruct(id).map(|v| v.0)
    }

    /// Attempt to find a component from the given id
    pub fn find_component<T: ComponentValue>(&self, id: ComponentId) -> Option<Component<T>> {
        let e = self.entity(id).ok()?;

        let info = e.get(is_component()).ok()?;

        if !info.is::<T>() {
            panic!("Attempt to construct a component from the wrong type");
        }
        // Safety: the type

        Some(unsafe { Component::from_raw_id(id, info.name(), info.meta()) })
    }

    /// Access, insert, and remove all components of an entity
    pub fn entity_mut(&mut self, id: Entity) -> Result<EntityRefMut> {
        let loc = self.location(id)?;
        Ok(EntityRefMut {
            world: self,
            loc,
            id,
        })
    }

    /// Access all components of an entity
    pub fn entity(&self, id: Entity) -> Result<EntityRef> {
        let loc = self.location(id)?;
        Ok(EntityRef {
            world: self,
            loc,
            id,
        })
    }

    /// Returns an entry for a given component of an entity allowing for
    /// in-place manipulation, insertion or removal.
    ///
    /// Fails if the entity is not valid.
    pub fn entry<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
    ) -> Result<Entry<T>> {
        let loc = self.location(id)?;
        let arch = self.archetypes.get(loc.arch);
        if arch.has(component.id()) {
            return Ok(Entry::Occupied(OccupiedEntry {
                borrow: self.get_mut(id, component).unwrap(),
            }));
        } else {
            return Ok(Entry::Vacant(VacantEntry {
                world: self,
                id,
                component,
            }));
        };
    }

    /// Subscribe for removals of a component.
    ///
    /// The affected entity and component will be transmitted when the component is removed,
    /// or when the entity is despawned.
    ///
    /// At consumption, the entity may not be alive
    pub fn on_removed<T: ComponentValue + Clone>(
        &mut self,
        component: Component<T>,
        tx: flume::Sender<(Entity, T)>,
    ) {
        let func = move |id: Entity, ptr: *const u8| unsafe {
            let val = ptr.cast::<T>().as_ref().expect("not null").clone();
            tx.send((id, val)).is_ok()
        };

        self.on_removed.register(component.id(), Box::new(func));
    }

    /// Merges `other` into `self`.
    ///
    /// Colliding entities will be migrated to a new entity id.
    ///
    /// Returns a map of all the entities which were remapped.
    ///
    /// `other` will be left empty
    ///
    /// **Note**: The data from `other` will all be marked as `inserted`
    /// as change events do not carry over.
    pub fn merge_with(&mut self, other: &mut World) -> Migrated {
        let mut archetypes = mem::replace(&mut other.archetypes, Archetypes::new());
        let mut entities = mem::take(&mut other.entities);

        let mut components = BTreeMap::new();

        let new_ids: BTreeMap<_, _> = archetypes
            .inner
            .iter_mut()
            .filter(|v| !v.1.has(is_static_component().id()))
            .flat_map(|(arch_id, arch)| {
                arch.entities_mut()
                    .iter_mut()
                    .filter_map(|id| {
                        let old_id = *id;

                        let loc = entities
                            .init(id.kind())
                            .get(*id)
                            .expect("Invalid component");

                        assert_eq!(loc.arch, arch_id);

                        if self.find_alive(id.low()).is_some() {
                            let new_id = self.spawn_inner(self.archetypes.root, id.kind()).0;

                            *id = new_id;
                            Some((old_id.low(), new_id))
                        } else {
                            self.spawn_at(old_id);
                            None
                        }
                    })
                    .collect_vec()
            })
            .collect();

        for (_, arch) in &mut archetypes.inner.iter_mut() {
            if arch.has(is_component().id()) {
                for (slot, &id) in arch.slots().iter().zip(arch.entities()) {
                    components.insert(
                        id.low(),
                        *arch.get(slot, is_component()).expect("Invalid slot"),
                    );
                }
            }

            // Don't migrate static components
            if !arch.has(is_static_component().id()) {
                let mut batch = BatchSpawn::new(arch.len());
                for mut storage in mem::take(arch.storage_mut()).into_values() {
                    let id = storage.info().id;

                    // Modify the relations to match new components
                    if id.is_relation() {
                        let (low, high) = id.split_pair();

                        let low = entities.init(low.kind()).reconstruct(low).unwrap().0;

                        let high = entities.init(high.kind()).reconstruct(high).unwrap().0;

                        let new_low = *new_ids.get(&low.low()).unwrap_or(&low);
                        let new_high = *new_ids.get(&high.low()).unwrap_or(&high);

                        // Safety
                        // The component is still of the same type
                        unsafe {
                            storage.set_id(Entity::pair(new_low, new_high));
                        }
                    } else {
                        let new_id = *new_ids.get(&id.low()).unwrap_or(&id);
                        // Safety
                        // The component is still of the same type
                        unsafe {
                            storage.set_id(new_id);
                        }
                    }

                    batch.insert(storage).expect("Batch is incomplete");
                }

                self.spawn_batch_at(arch.entities(), &mut batch)
                    .expect("Failed to spawn batch")
            }
        }

        Migrated {
            ids: new_ids,
            components,
        }
    }
}

/// Holds the migrated components
#[derive(Debug, Clone)]
pub struct Migrated {
    ids: BTreeMap<StrippedEntity, Entity>,
    components: BTreeMap<StrippedEntity, ComponentInfo>,
}

impl Migrated {
    /// Retuns the new id if it was migrated, otherwise, returns the given id
    pub fn get(&self, id: Entity) -> Entity {
        *self.ids.get(&id.low()).unwrap_or(&id)
    }

    /// Returns the migrated component. All components are migrated
    /// # Panics
    /// If the types do not match
    pub fn get_component<T: ComponentValue>(&self, component: Component<T>) -> Component<T> {
        let id = self.get(component.id());

        let mut info = *self
            .components
            .get(&id.low())
            .expect("{component} is not a component or not present in the world");

        info.id = id;

        if !info.is::<T>() {
            panic!("Mismatched component types {component:?} for {info:#?}");
        }

        unsafe { Component::from_raw_id(info.id(), info.name(), info.meta()) }
    }

    /// Returns the migrated relation
    /// # Panics
    /// If the types do not match
    pub fn get_relation<T: ComponentValue>(
        &self,
        relation: impl RelationExt<T>,
    ) -> impl Fn(Entity) -> Component<T> {
        let id = relation.of(wildcard()).id();
        let id = *self.ids.get(&id.low()).unwrap_or(&id);

        let mut info = *self
            .components
            .get(&id.low())
            .expect("relation is not a component or not present in the world");

        info.id = id;

        if !info.is::<T>() {
            panic!("Mismatched relation types");
        }

        move |object| unsafe { Component::from_pair(info.id(), info.name(), info.meta(), object) }
    }

    /// Returns the migrated ids
    pub fn ids(&self) -> &BTreeMap<StrippedEntity, Entity> {
        &self.ids
    }
}

/// Debug formats the world with the given filter.
/// Created using [World::format_debug]
pub struct WorldFormatter<'a, F> {
    world: &'a World,
    filter: F,
}

impl<'a, F> std::fmt::Debug for WorldFormatter<'a, F>
where
    F: for<'x> Filter<'x>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut meta = BTreeMap::new();
        let mut list = f.debug_map();

        let mut query = Query::new(()).with_components().filter(&self.filter);
        let mut query = query.borrow(self.world);

        for batch in query.iter_batched() {
            let arch = batch.arch();
            meta.clear();
            meta.extend(arch.components().flat_map(|info| {
                Some((info.id(), self.world.get(info.id(), debug_visitor()).ok()?))
            }));

            for slot in batch.slots().iter() {
                assert!(
                    slot < arch.len(),
                    "batch is larger than archetype, batch: {:?}, arch: {:?}",
                    batch.slots(),
                    arch.entities()
                );

                let row = RowFormatter::new(arch, slot, &meta);
                list.entry(&arch.entity(slot).unwrap(), &row);
            }
        }

        list.finish()
    }
}

/// Debug formats the specified entities,
/// Created using [World::format_entities]
pub struct EntityFormatter<'a> {
    world: &'a World,
    ids: &'a [Entity],
}

impl<'a> std::fmt::Debug for EntityFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut meta = BTreeMap::new();
        let mut list = f.debug_map();

        for &id in self.ids {
            let loc = self.world.location(id);
            if let Ok(loc) = loc {
                let arch = self.world.archetypes.get(loc.arch);

                meta.extend(arch.components().flat_map(|info| {
                    Some((info.id(), self.world.get(info.id(), debug_visitor()).ok()?))
                }));

                let row = RowFormatter::new(arch, loc.slot, &meta);
                list.entry(&id, &row);
            }
        }

        list.finish()
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.format_debug(is_component().without()).fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{component, EntityBuilder, Query};

    use super::*;

    component! {
        a: i32,
        b: f32,
        c: String,
        d: Vec<u32>,
        e: Arc<String>,
    }

    #[test]
    fn world_archetype_graph() {
        let mut world = World::new();

        // () -> (a) -> (ab) -> (abc)
        let (_, archetype) = world.archetypes.init([a().info(), b().info(), c().info()]);
        assert!(!archetype.has(d().id()));
        assert!(archetype.has(a().id()));
        assert!(archetype.has(b().id()));

        // () -> (a) -> (ab) -> (abc)
        //                   -> (abd)
        let (_, archetype) = world.archetypes.init([a().info(), b().info(), d().info()]);
        assert!(archetype.has(d().id()));
        assert!(!archetype.has(c().id()));
    }

    #[test]
    fn insert() {
        let mut world = World::new();
        let id = world.spawn();

        world.set(id, a(), 65).unwrap();
        let shared = Arc::new("Foo".to_string());

        assert_eq!(world.get(id, a()).as_deref(), Ok(&65));
        assert_eq!(
            world.get(id, b()).as_deref(),
            Err(&Error::MissingComponent(id, "b"))
        );
        assert!(!world.has(id, c()));

        let id2 = world.spawn();
        world.set(id2, a(), 7).unwrap();

        world.set(id2, c(), "Foo".to_string()).unwrap();

        eprintln!("a: {}, b: {}, c: {}, id: {}", a(), a(), c(), id);

        assert_eq!(world.get(id, a()).as_deref(), Ok(&65));
        assert_eq!(
            world.get(id, b()).as_deref(),
            Err(&Error::MissingComponent(id, "b"))
        );

        assert!(!world.has(id, c()));

        assert_eq!(world.get(id2, a()).as_deref(), Ok(&7));
        assert_eq!(world.get(id2, c()).as_deref(), Ok(&"Foo".to_string()));
        world.set(id, e(), shared.clone()).unwrap();
        assert_eq!(
            world.get(id, e()).as_deref().map(|v| &**v),
            Ok(&"Foo".to_string())
        );

        assert_eq!(Arc::strong_count(&shared), 2);
        drop(world);
        assert_eq!(Arc::strong_count(&shared), 1);
    }

    #[test]
    fn concurrent_borrow() {
        let mut world = World::new();
        let id1 = world.spawn();
        let id2 = world.spawn();

        world.set(id1, a(), 40).unwrap();

        world.set(id2, b(), 4.3).unwrap();

        // Borrow a
        let id_a = world.get(id1, a()).unwrap();
        assert_eq!(*id_a, 40);
        // Borrow b uniquely while a is in scope
        let mut id2_b = world.get_mut(id2, b()).unwrap();

        *id2_b = 3.21;

        assert_eq!(*id_a, 40);

        // Borrow another component on an entity with a mutable borrowed
        // **other** component.
        assert_eq!(world.get(id2, a()).as_deref().ok(), None);
    }

    #[test]
    fn remove() {
        let mut world = World::new();
        let id = EntityBuilder::new()
            .set(a(), 9)
            .set(b(), 0.3)
            .set(c(), "Foo".to_string())
            .spawn(&mut world);

        let shared = Arc::new("The meaning of life is ...".to_string());

        world.set(id, e(), shared.clone()).unwrap();
        let id2 = EntityBuilder::new()
            .set(a(), 6)
            .set(b(), 0.219)
            .set(c(), "Bar".to_string())
            .set(e(), shared.clone())
            .spawn(&mut world);

        assert_eq!(world.get(id, b()).as_deref(), Ok(&0.3));
        assert_eq!(world.get(id, e()).as_deref(), Ok(&shared));

        assert_eq!(world.remove(id, e()).as_ref(), Ok(&shared));

        assert_eq!(world.get(id, a()).as_deref(), Ok(&9));
        assert_eq!(world.get(id, c()).as_deref(), Ok(&"Foo".to_string()));
        assert_eq!(
            world.get(id, e()).as_deref(),
            Err(&Error::MissingComponent(id, "e"))
        );

        world.despawn(id).unwrap();

        assert_eq!(world.get(id, a()).as_deref(), Err(&Error::NoSuchEntity(id)));
        assert_eq!(world.get(id, c()).as_deref(), Err(&Error::NoSuchEntity(id)));
        assert_eq!(world.get(id, e()).as_deref(), Err(&Error::NoSuchEntity(id)));

        assert_eq!(world.get(id2, e()).as_deref(), Ok(&shared));
        assert_eq!(world.get(id2, c()).as_deref(), Ok(&"Bar".to_string()));

        assert_eq!(world.get(id, e()).as_deref(), Err(&Error::NoSuchEntity(id)));

        assert_eq!(Arc::strong_count(&shared), 2);

        // // Remove id

        let mut query = Query::new((a(), c()));
        let items = query.as_vec(&world);

        assert_eq!(items, [(6, "Bar".to_string())]);
    }
}
