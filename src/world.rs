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
    components::{is_component, name},
    debug_visitor, entities,
    entity::{EntityLocation, EntityStore},
    entity_ref::{EntityRef, EntityRefMut},
    entry::{Entry, OccupiedEntry, VacantEntry},
    error::Result,
    Component, ComponentBuffer, ComponentId, ComponentValue, Entity, EntityKind, EntityStoreIter,
    EntityStoreIterMut, Error, Filter, Query, RelationExt, RowFormatter, StaticFilter,
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
    archetypes: EntityStore<Archetype>,
    gen: AtomicU32,
}

impl Archetypes {
    pub fn new() -> Self {
        let mut archetypes = EntityStore::new(EntityKind::empty());
        let root = archetypes.spawn(Archetype::empty());
        Self {
            root,
            archetypes,
            gen: AtomicU32::new(0),
        }
    }

    pub fn get(&self, arch_id: ArchetypeId) -> &Archetype {
        self.archetypes.get(arch_id).expect("Invalid archetype")
    }

    pub fn get_mut(&mut self, arch_id: ArchetypeId) -> &mut Archetype {
        self.archetypes.get_mut(arch_id).expect("Invalid archetype")
    }

    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    pub(crate) fn init<'a>(
        &mut self,
        components: impl IntoIterator<Item = &'a ComponentInfo>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut cursor = self.root;

        for head in components {
            let cur = &mut self.archetypes.get(cursor).unwrap();
            cursor = match cur.edge_to(head.id) {
                Some(id) => id,
                None => {
                    let new = Archetype::new(cur.components().copied().chain([*head]));

                    tracing::debug!(
                        "Creating new archetype: {:#?}",
                        new.components().collect_vec()
                    );

                    // Increase gen
                    self.gen.fetch_add(1, Ordering::Relaxed);
                    let id = self.archetypes.spawn(new);

                    let (cur, new) = self.archetypes.get_disjoint(cursor, id).unwrap();
                    cur.add_edge_to(new, id, cursor, head.id);

                    id
                }
            };
        }

        (cursor, self.archetypes.get_mut(cursor).unwrap())
    }

    pub fn root(&mut self) -> &mut Archetype {
        self.get_mut(self.root)
    }

    pub fn get_disjoint(
        &mut self,
        a: Entity,
        b: Entity,
    ) -> Option<(&mut Archetype, &mut Archetype)> {
        self.archetypes.get_disjoint(a, b)
    }

    pub fn iter(&self) -> EntityStoreIter<Archetype> {
        self.archetypes.iter()
    }

    pub fn iter_mut(&mut self) -> EntityStoreIterMut<Archetype> {
        self.archetypes.iter_mut()
    }

    pub fn despawn(&mut self, id: Entity) -> Result<Archetype> {
        self.archetypes.despawn(id)
    }
}

/// Holds the entities and components of the ECS.
pub struct World {
    entities: EntityStores,
    pub(crate) archetypes: Archetypes,
    change_tick: AtomicU32,
}

impl World {
    /// Creates a new empty world
    pub fn new() -> Self {
        Self {
            entities: EntityStores::default(),
            archetypes: Archetypes::new(),
            change_tick: AtomicU32::new(0),
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
                .expect("failed to initialize component");
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

        let arch = self.archetype_mut(arch_id);

        for (_, mut storage) in batch.take_all() {
            unsafe {
                arch.extend(&mut storage)
                    .expect("Component not in archetype");
            }

            arch.init_changes(*storage.info())
                .set(Change::inserted(slots, change_tick));
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

        for &id in ids {
            assert!(matches!(
                self.despawn(id),
                Err(Error::NoSuchEntity(..)) | Ok(())
            ));
        }

        for &component in batch.components() {
            self.init_component(component)
                .expect("failed to initialize component");
        }

        let change_tick = self.advance_change_tick();

        let (arch_id, arch) = self.archetypes.init(batch.components());

        let base = arch.len();
        for (idx, &id) in ids.iter().enumerate() {
            let kind = id.kind();
            let store = self.entities.init(kind);
            store
                .spawn_at(
                    id.index(),
                    id.generation(),
                    EntityLocation {
                        slot: base + idx,
                        arch: arch_id,
                    },
                )
                .expect("Entity already exists");
        }

        let slots = arch.allocate_n(ids);

        let arch = self.archetype_mut(arch_id);

        for (_, mut storage) in batch.take_all() {
            unsafe {
                arch.extend(&mut storage)
                    .expect("Component not in archetype");
            }

            arch.init_changes(*storage.info())
                .set(Change::inserted(slots, change_tick));
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

        Component::new(id, name, meta)
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

        move |object| Component::new_pair(id, name, meta, object)
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

    /// Access an archetype by id
    pub fn archetype(&self, id: ArchetypeId) -> &Archetype {
        self.archetypes.get(id)
    }

    /// Access an archetype by id
    pub fn archetype_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        self.archetypes.get_mut(id)
    }

    /// Spawn an entity with the given components.
    ///
    /// For increased ergonomics, prefer [crate::EntityBuilder]
    pub fn spawn_at_with(&mut self, id: Entity, buffer: &mut ComponentBuffer) -> Entity {
        let change_tick = self.advance_change_tick();

        for component in buffer.components() {
            self.init_component(*component)
                .expect("Failed to initialize component");
        }

        let (arch_id, _) = self.archetypes.init(buffer.components());
        let (loc, arch) = self.spawn_at_inner(id, arch_id);

        for (component, src) in buffer.take_all() {
            unsafe {
                arch.push(component.id, src)
                    .expect("Component not in archetype")
            }

            arch.init_changes(component)
                .set(Change::inserted(Slice::single(loc.slot), change_tick));
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
                .set(Change::inserted(Slice::single(loc.slot), change_tick));
        }

        id
    }

    /// Removes all components from an entity without despawning the entity
    pub fn clear(&mut self, id: Entity) -> Result<()> {
        let EntityLocation { arch, slot } = self.location(id)?;

        let src = self.archetype_mut(arch);

        let swapped = unsafe { src.take(slot, |c, p| (c.drop)(p)) };
        let ns = self.entities.init(id.kind());
        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        *self.location_mut(id).unwrap() = EntityLocation {
            slot: self.archetypes.root().allocate(id),
            arch: self.archetypes.root,
        };

        self.detach(id);
        Ok(())
    }

    /// Sets a group of components for a single entity using an iterator of type
    /// erased data.
    ///
    /// # Safety
    /// The data pointed to by each item in the iterator must be a valid
    /// instance of the provided ComponentInfo.
    ///
    /// The data is considered moved and can not be used afterwards
    pub fn set_with(
        &mut self,
        id: Entity,
        components: impl IntoIterator<Item = (ComponentInfo, *mut u8)>,
    ) -> Result<()> {
        let id: Entity = id;
        let change_tick = self.advance_change_tick();

        let EntityLocation { arch, slot } = self.location(id)?;

        let mut new_data = Vec::new();
        let mut new_components = Vec::new();

        let src_id = arch;

        for (component, data) in components {
            let src = self.archetype_mut(arch);

            if let Some(old) = src.get_dyn(slot, component.id) {
                // Drop old and copy the new value in
                unsafe {
                    (component.drop)(old);
                    ptr::copy_nonoverlapping(data, old, component.size());
                }

                src.changes_mut(component.id())
                    .unwrap()
                    .set(Change::modified(Slice::single(slot), change_tick));
            } else {
                // Component does not exist yet, so defer a move

                // Data will have a lifetime of `components`.
                new_data.push((component, data));
                new_components.push(component);
            }
        }

        if !new_data.is_empty() {
            debug_assert_eq!(new_data.len(), new_components.len());
            let src = self.archetype_mut(arch);
            new_components.extend(src.components().copied());

            // Make sure everything is in its order
            new_components.sort_unstable();

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
                        .set(Change::inserted(Slice::single(dst_slot), change_tick));
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
            self.init_component(component)?;
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

        self.spawn_at(info.id());
        self.set_with(info.id(), meta.take_all()).unwrap();

        Ok(info)
    }

    /// Despawn an entity.
    /// Any relations to other entities will be removed.
    pub fn despawn(&mut self, id: Entity) -> Result<()> {
        let EntityLocation {
            arch: archetype,
            slot,
        } = self.location(id)?;

        let src = self.archetype_mut(archetype);

        let swapped = unsafe { src.take(slot, |c, p| (c.drop)(p)) };

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

    /// Despawns all components which matches the filter
    pub fn despawn_all<F>(&mut self, filter: F)
    where
        F: for<'x> Filter<'x>,
    {
        let mut query = Query::new(entities()).filter(filter);
        let ids = query.iter(self).iter().collect_vec();

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
                .iter_mut()
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
            let mut src = self.archetypes.despawn(src).unwrap();

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

    pub(crate) fn set_inner<T: ComponentValue>(
        &mut self,
        id: impl Into<Entity>,
        component: Component<T>,
        mut value: T,
    ) -> Result<(Option<T>, EntityLocation)> {
        let id = id.into();
        // We know things will change either way
        let change_tick = self.advance_change_tick();

        let EntityLocation { arch: src_id, slot } = self.location(id)?;

        let component_info = component.info();

        // if let Some(&EntityLocation { arch: src_id, slot }) = loc {
        let src = self.archetype(src_id);

        if let Some(mut val) = src.get_mut(slot, component) {
            src.changes_mut(component.id())
                .expect("Missing change list")
                .set(Change::modified(Slice::single(slot), change_tick));

            return Ok((
                Some(mem::replace(&mut *val, value)),
                EntityLocation { arch: src_id, slot },
            ));
        }

        // Pick up the entity and move it to the destination archetype
        let dst_id = match src.edge_to(component.id()) {
            Some(dst) => dst,
            None => {
                let pivot = src
                    .components()
                    .take_while(|v| v.id < component.id())
                    .count();

                // Split the components
                // A B C [new] D E F
                let left = src.components().take(pivot).copied();
                let right = src.components().skip(pivot).copied();

                let components: Vec<_> = left.chain([component_info]).chain(right).collect();

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
            src.add_edge_to(dst, dst_id, src_id, component.id());

            // Insert the missing component
            dst.push(component_info.id, &mut value as *mut T as *mut u8)
                .expect("Insert should not fail");

            // And don't forget to forget to drop it
            std::mem::forget(value);

            debug_assert_eq!(dst.entity(dst_slot), Some(id));

            dst.init_changes(component.info())
                .set(Change::inserted(Slice::single(dst_slot), change_tick));

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

            self.init_component(component.info())?;

            Ok((None, loc))
        }
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

        let src = self.archetype(src_id);

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
            (drop)(p);
        });

        assert_eq!(dst.entity(dst_slot), Some(id));

        // Migrate all changes
        dst.init_changes(component)
            .set(Change::removed(Slice::single(dst_slot), change_tick));

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

        self.archetype(loc.arch)
            .get(loc.slot, component)
            .ok_or_else(|| Error::MissingComponent(id, component.name()))
    }

    pub(crate) fn get_at<T: ComponentValue>(
        &self,
        EntityLocation { arch, slot }: EntityLocation,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        self.archetype(arch).get(slot, component)
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
        let archetype = self.archetype(arch);

        let change_tick = self.advance_change_tick();

        if !archetype.has(component.id()) {
            return None;
        }

        archetype
            .changes_mut(component.id())
            .expect("Change list is empty")
            .set(Change::modified(Slice::single(slot), change_tick));

        archetype.get_mut(slot, component)
    }

    /// Returns true if the entity has the specified component.
    /// Returns false if the entity does not exist or it does not have the
    /// specified component
    pub fn has<T: ComponentValue>(&self, id: Entity, component: Component<T>) -> bool {
        if let Ok(loc) = self.location(id) {
            self.archetype(loc.arch).has(component.id())
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
        self.archetypes.gen.load(Ordering::Relaxed)
    }

    #[must_use]
    /// Returns the current world change tick
    pub fn change_tick(&self) -> u32 {
        self.change_tick.load(Ordering::Relaxed)
    }

    /// Increases the change tick and returns the new one
    pub fn advance_change_tick(&self) -> u32 {
        self.change_tick.fetch_add(1, Ordering::Relaxed) + 1
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

    pub(crate) fn reconstruct(&self, id: crate::StrippedEntity) -> Option<Entity> {
        let ns = self.entities.get(id.kind())?;

        ns.reconstruct(id).map(|v| v.0)
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
        let arch = self.archetype(loc.arch);
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

        let mut query = Query::with_components(()).filter(&self.filter);
        let mut query = query.iter(self.world);

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
                let arch = self.world.archetype(loc.arch);

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
        let (_, archetype) = world.archetypes.init(&[a().info(), b().info(), c().info()]);
        assert!(!archetype.has(d().id()));
        assert!(archetype.has(a().id()));
        assert!(archetype.has(b().id()));

        // () -> (a) -> (ab) -> (abc)
        //                   -> (abd)
        let (_, archetype) = world.archetypes.init(&[a().info(), b().info(), d().info()]);
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
