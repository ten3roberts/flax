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
    archetype::{Archetype, ArchetypeId, Change, ComponentBatch, ComponentInfo, Slice},
    components::{component, name},
    debug_visitor, entities,
    entity::{EntityLocation, EntityStore},
    entity_ref::{EntityRef, EntityRefMut},
    entry::{Entry, OccupiedEntry, VacantEntry},
    error::Result,
    Component, ComponentBuffer, ComponentId, ComponentValue, Entity, EntityKind, Error, Filter,
    Query, RowFormatter, StaticFilter,
};

/// Holds the entities and components of the ECS.
pub struct World {
    entities: BTreeMap<EntityKind, EntityStore>,
    pub(crate) archetypes: EntityStore<Archetype>,
    archetype_root: ArchetypeId,
    change_tick: AtomicU32,
    archetype_gen: AtomicU32,
}

impl World {
    pub fn new() -> Self {
        let mut archetypes = EntityStore::new(EntityKind::empty());
        let root = archetypes.spawn(Archetype::empty());

        Self {
            entities: BTreeMap::new(),
            archetypes,
            change_tick: AtomicU32::new(0),
            archetype_gen: AtomicU32::new(0),
            archetype_root: root,
        }
    }

    pub fn get_store(&self, kind: EntityKind) -> Result<&EntityStore> {
        self.entities.get(&kind).ok_or(Error::NoSuchKind(kind))
    }

    pub fn init_store(&mut self, kind: EntityKind) -> &mut EntityStore {
        self.entities
            .entry(kind)
            .or_insert_with(|| EntityStore::new(kind))
    }

    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    pub fn find_archetype(
        &self,
        root: ArchetypeId,
        mut components: &[ComponentId],
    ) -> Option<&Archetype> {
        let mut cursor = root;

        while let [head, tail @ ..] = components {
            let next = self.archetypes.get(cursor).unwrap().edge_to(*head)?;
            cursor = next;
            components = tail;
        }

        self.archetypes.get(cursor)
    }

    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    pub(crate) fn fetch_archetype<'a>(
        &mut self,
        components: impl IntoIterator<Item = &'a ComponentInfo>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut cursor = self.archetype_root;

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
                    self.archetype_gen.fetch_add(1, Ordering::Relaxed);
                    let id = self.archetypes.spawn(new);

                    let (cur, new) = self.archetypes.get_disjoint(cursor, id).unwrap();
                    cur.add_edge_to(new, id, cursor, head.id);

                    id
                }
            };
        }

        (cursor, self.archetypes.get_mut(cursor).unwrap())
    }

    pub fn spawn_many(&mut self) -> impl Iterator<Item = Entity> + '_ {
        (0..).map(|_| self.spawn())
    }

    /// Spawn a new empty entity into the default namespace
    pub fn spawn(&mut self) -> Entity {
        self.spawn_inner(self.archetype_root, EntityKind::empty()).0
    }

    pub fn spawn_batch(&mut self, batch: ComponentBatch) -> Vec<Entity> {
        let ids = self.spawn_many().take(batch.len()).collect_vec();
        self.spawn_batch_at(&ids, batch);
        ids
    }

    /// Batch spawn multiple components with prespecified ids.
    pub fn spawn_batch_at(&mut self, ids: &[Entity], batch: ComponentBatch) -> Result<()> {
        assert_eq!(
            ids.len(),
            batch.len(),
            "The length of ids must match the number of slots in `batch`"
        );

        // for id in ids {
        //     self.spawn_at(id)?;
        // }

        let (arch_id, arch) = self.fetch_archetype(batch.components());

        let slots = arch.allocate_n(ids);

        for (id, mut storage) in batch.take_all() {
            unsafe {
                arch.extend(&mut storage)
                    .expect("Component not in archetype");
            }
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
        let (id, _) = self.spawn_inner(self.archetype_root, EntityKind::COMPONENT);
        Component::new(id, name, meta)
    }

    fn spawn_inner(&mut self, arch_id: ArchetypeId, kind: EntityKind) -> (Entity, EntityLocation) {
        // Place at root
        let ns = {
            self.entities
                .entry(kind)
                .or_insert_with(|| EntityStore::new(kind))
        };

        let arch = {
            self.archetypes
                .get_mut(arch_id)
                .expect("Archetype does not exist")
        };

        let slot = arch.len();

        let loc = EntityLocation {
            arch: arch_id,
            slot,
        };
        let id = ns.spawn(loc);

        arch.allocate(id);

        (id, loc)
    }

    /// Spawns an entitiy with a specific id.
    /// Fails if an entity with the same index already exists.
    pub fn spawn_at(&mut self, id: Entity) -> Result<()> {
        self.spawn_inner_at(id, self.archetype_root)?;
        Ok(())
    }

    fn spawn_inner_at(&mut self, id: Entity, arch_id: ArchetypeId) -> Result<EntityLocation> {
        let kind = id.kind();
        let ns = {
            self.entities
                .entry(kind)
                .or_insert_with(|| EntityStore::new(kind))
        };

        let arch = {
            self.archetypes
                .get_mut(arch_id)
                .expect("Archetype does not exist")
        };
        let slot = arch.len();

        let location = ns.spawn_at(
            id.index(),
            id.generation(),
            EntityLocation {
                slot,
                arch: arch_id,
            },
        )?;

        arch.allocate(id);
        Ok(*location)
    }

    /// Access an archetype by id
    pub fn archetype(&self, id: ArchetypeId) -> &Archetype {
        self.archetypes.get(id).expect("Archetype does not exist")
    }

    /// Access an archetype by id
    pub fn archetype_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        self.archetypes
            .get_mut(id)
            .expect("Archetype does not exist")
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

        let (arch_id, _) = self.fetch_archetype(buffer.components());
        let (id, loc) = self.spawn_inner(arch_id, EntityKind::empty());

        let arch = self.archetype_mut(arch_id);
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
        let ns = self.init_store(id.kind());
        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        let root = self.archetype_root;

        *self.location_mut(id).unwrap() = EntityLocation {
            slot: self.archetype_mut(root).allocate(id),
            arch: root,
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
    pub unsafe fn set_with(
        &mut self,
        id: impl Into<Entity>,
        components: impl IntoIterator<Item = (ComponentInfo, *mut u8)>,
    ) -> Result<()> {
        let id: Entity = id.into();
        let change_tick = self.advance_change_tick();

        let EntityLocation { arch, slot } = self.location(id)?;

        let mut new_data = Vec::new();
        let mut new_components = Vec::new();

        let src_id = arch;

        for (component, data) in components {
            let src = self.archetype_mut(arch);

            if let Some(old) = src.get_dyn(slot, component.id) {
                // Drop old
                (component.drop)(old);
                ptr::copy_nonoverlapping(data, old, component.size());

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

            let (dst_id, _) = self.fetch_archetype(components.iter());

            // Borrow disjoint
            let (src, dst) = self.archetypes.get_disjoint(src_id, dst_id).unwrap();

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
                self.init_store(swapped.kind())
                    .get_mut(swapped)
                    .expect("Invalid entity id")
                    .slot = slot;
            }

            *self.location_mut(id).expect("Entity is not valid") = EntityLocation {
                slot: dst_slot,
                arch: dst_id,
            };

            for (component, _) in new_data {
                self.init_component(component)?;
            }
        }

        Ok(())
    }

    /// Set metadata for a given component if they do not already exist
    fn init_component(&mut self, info: ComponentInfo) -> Result<ComponentInfo> {
        if self.is_alive(info.id()) {
            return Ok(info);
        }

        let mut meta = info.get_meta();

        let loc = self.init_location(info.id())?;

        debug_assert_eq!(Ok(loc), self.location(info.id()));

        unsafe { self.set_with(info.id(), meta.take_all()).unwrap() }

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

        let ns = self.init_store(id.kind());
        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        ns.despawn(id)?;
        self.detach(id);
        Ok(())
    }

    /// Despawns all components which matches the filter
    pub fn despawn_all<F>(&mut self, filter: F)
    where
        F: StaticFilter,
    {
        let mut query = Query::new(entities()).filter(filter);
        let ids = query.prepare(self).iter().collect_vec();

        for id in ids {
            self.despawn(id).expect("Invalid entity id");
        }
    }

    /// Despawns an entity and all connected entities given by the supplied
    /// relations.
    pub fn despawn_recursive(
        &mut self,
        id: Entity,
        relations: &[fn(Entity) -> ComponentId],
    ) -> Result<()> {
        let mut to_remove = vec![id];

        while let Some(id) = to_remove.pop() {
            self.despawn(id)?;

            for (_, arch) in self
                .archetypes
                .iter_mut()
                .filter(|(_, arch)| relations.iter().map(|v| v(id)).any(|v| arch.has(v)))
            {
                to_remove.extend_from_slice(arch.entities());
            }
        }

        Ok(())
    }

    /// Removes all instances of relations and component of the given entities
    /// in the world. If used upon an entity with a child -> parent relation, this removes the relation
    /// on all the children.
    pub fn detach(&mut self, component: ComponentId) {
        let subject = component.low();
        // The archetypes to remove
        let archetypes = self
            .archetypes()
            .filter(|(_, v)| {
                v.has(component)
                    || v.relations()
                        .any(|id| id.low() == subject || id.high() == subject)
            })
            .map(|v| v.0)
            .collect_vec();

        for src in archetypes {
            let mut src = self.archetypes.despawn(src).unwrap();

            let components = src.components().filter(|info| {
                let id = info.id();
                !(id == component
                    || (id.is_relation() && (id.low() == subject || id.high() == subject)))
            });

            let (dst_id, dst) = self.fetch_archetype(components);

            eprintln!("{:?} => {dst_id}", src.component_names().collect_vec());

            for (id, slot) in src.move_all(dst) {
                *self.location_mut(id).expect("Entity id was not valid") =
                    EntityLocation { slot, arch: dst_id }
            }
        }
    }

    pub fn set<T: ComponentValue>(
        &mut self,
        id: impl Into<Entity>,
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
                let (dst_id, _) = self.fetch_archetype(&components);

                dst_id
            }
        };

        eprintln!("Moving {}", component.name());
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
                let swapped_ns = self.init_store(swapped.kind());
                swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
            }

            let ns = self.init_store(id.kind());
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
        eprintln!("Removing component {component:?} from {id} ");
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
        let ns = self.init_store(id.kind());
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

                let (dst_id, _) = self.fetch_archetype(&components);

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

        eprintln!("Moving {id} from {src_id} => {dst_id}");
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
            let swapped_ns = self.init_store(swapped.kind());
            swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        let loc = EntityLocation {
            slot: dst_slot,
            arch: dst_id,
        };

        *self.location_mut(id).expect("Entity is not valid") = loc;

        Ok(loc)
    }

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
        id: impl Into<Entity>,
        component: Component<T>,
    ) -> Result<AtomicRef<T>> {
        let id = id.into();
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
        self.get_store(id.kind())
            .map(|v| v.is_alive(id))
            .unwrap_or(false)
    }

    pub(crate) fn archetypes(&self) -> impl Iterator<Item = (ArchetypeId, &Archetype)> {
        self.archetypes.iter()
    }

    /// Returns the location of an entity, or spawns it if is in the static
    /// namespace.
    ///
    /// This is often the case when setting components to components.
    ///
    /// If the entity is not found and is not in the static namespace an error
    /// will be returned.
    pub(crate) fn init_location(&mut self, id: Entity) -> Result<EntityLocation> {
        self.init_store(id.kind())
            .get(id)
            .ok_or(Error::NoSuchEntity(id))
            .copied()
            .or_else(|_| self.spawn_inner_at(id, self.archetype_root))
    }

    /// Returns the location inside an archetype for a given entity
    pub fn location(&self, id: Entity) -> Result<EntityLocation> {
        self.get_store(id.kind())?
            .get(id)
            .ok_or(Error::NoSuchEntity(id))
            .copied()
    }

    fn location_mut(&mut self, id: Entity) -> Result<&mut EntityLocation> {
        self.init_store(id.kind())
            .get_mut(id)
            .ok_or(Error::NoSuchEntity(id))
    }

    /// Get a reference to the world's archetype generation
    #[must_use]
    pub fn archetype_gen(&self) -> u32 {
        self.archetype_gen.load(Ordering::Relaxed)
    }

    #[must_use]
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

    // /// Visit all components which have the visitor components and use the
    // /// associated visitor for each slot
    // pub fn visit<C, V, Ret>(&self, visitor: Component<V>, ctx: &mut C) -> impl Iterator<Item = Ret>
    // where
    //     V: Visitor<C, Ret> + ComponentValue,
    // {
    //     for (_, arch) in self.archetypes.iter() {
    //         for component in arch.components() {
    //             // eprintln!("Traversing: {}:{}", component.name(), component.id());
    //             if let Ok(mut v) = self.get_mut(component.id(), visitor) {
    //                 // eprintln!("Visiting {}{}", component.name(), component.id());
    //                 arch.visit(component.id(), &mut *v, ctx);
    //             }
    //         }
    //     }
    // }

    pub fn component_metadata(&self) -> BTreeMap<ComponentInfo, Vec<String>> {
        let filter = component().with();
        self.archetypes
            .iter()
            .filter(|(_, arch)| filter.matches(arch))
            .map(|(_, arch)| {
                (
                    arch.slots(),
                    arch.storage(component()).unwrap(),
                    arch.components()
                        .map(|v| v.name().to_string())
                        .collect_vec(),
                )
            })
            .flat_map(|(slots, keys, values)| {
                slots.iter().map(move |v| {
                    let info = keys[v];
                    (info, values.clone())
                })
            })
            .collect()
    }

    pub(crate) fn reconstruct(&self, id: crate::StrippedEntity) -> Option<Entity> {
        let ns = self.get_store(id.kind()).ok()?;

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

pub struct WorldFormatter<'a, F> {
    world: &'a World,
    filter: F,
}

impl<'a, F> std::fmt::Debug for WorldFormatter<'a, F>
where
    F: StaticFilter,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut meta = BTreeMap::new();
        let mut list = f.debug_map();

        let mut query = Query::with_components(()).filter(&self.filter);
        let mut query = query.prepare(self.world);

        for batch in query.iter_batched() {
            let arch = batch.arch();
            meta.clear();
            meta.extend(arch.components().flat_map(|info| {
                Some((
                    info.id(),
                    (
                        self.world.get(info.id(), debug_visitor()).ok(),
                        self.world.get(info.id(), name()).ok()?,
                    ),
                ))
            }));

            for slot in batch.slots().iter() {
                let row = RowFormatter::new(arch, slot, &meta);
                list.entry(&arch.entity(slot).unwrap(), &row);
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
        self.format_debug(component().without()).fmt(f)
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
        let (_, archetype) = world.fetch_archetype(&[a().info(), b().info(), c().info()]);
        assert!(!archetype.has(d().id()));
        assert!(archetype.has(a().id()));
        assert!(archetype.has(b().id()));

        // () -> (a) -> (ab) -> (abc)
        //                   -> (abd)
        let (_, archetype) = world.fetch_archetype(&[a().info(), b().info(), d().info()]);
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
