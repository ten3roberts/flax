use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::{
    fmt,
    fmt::Formatter,
    mem::{self, MaybeUninit},
    sync::atomic::{AtomicBool, AtomicU32, Ordering, Ordering::Relaxed},
};
use once_cell::unsync::OnceCell;
use smallvec::SmallVec;

use atomic_refcell::{AtomicRef, BorrowError, BorrowMutError};
use itertools::Itertools;

use crate::{
    archetype::{Archetype, ArchetypeId, ArchetypeInfo, Slot},
    archetypes::Archetypes,
    buffer::ComponentBuffer,
    component::{dummy, ComponentDesc, ComponentKey, ComponentValue},
    components::{self, component_info, is_static_entity, name},
    entity::{entity_ids, Entity, EntityIndex, EntityKind, EntityLocation, EntityStore},
    entity_ref::{EntityRef, EntityRefMut},
    entry::{Entry, OccupiedEntry, VacantEntry},
    error::{MissingComponent, Result},
    events::EventSubscriber,
    filter::StaticFilter,
    format::{EntitiesFormatter, HierarchyFormatter, WorldFormatter},
    relation::{Relation, RelationExt},
    writer::{
        self, EntityWriter, FnWriter, Replace, ReplaceDyn, SingleComponentWriter, WriteDedup,
    },
    BatchSpawn, Component, ComponentVTable, Error, Fetch, Query, RefMut,
};

#[derive(Debug, Default)]
struct EntityStores {
    inner: BTreeMap<EntityKind, EntityStore>,
}

impl EntityStores {
    fn new() -> Self {
        Self {
            inner: BTreeMap::from([(EntityKind::empty(), EntityStore::new(EntityKind::empty()))]),
        }
    }

    fn init(&mut self, kind: EntityKind) -> &mut EntityStore {
        self.inner
            .entry(kind)
            .or_insert_with(|| EntityStore::new(kind))
    }

    fn get(&self, kind: EntityKind) -> Option<&EntityStore> {
        self.inner.get(&kind)
    }
}

pub(crate) fn update_entity_loc(
    world: &mut World,
    id: Entity,
    loc: EntityLocation,
    swapped: Option<(Entity, Slot)>,
) {
    if let Some((swapped, slot)) = swapped {
        // The last entity in src was moved into the slot occupied by id
        let swapped_ns = world.entities.init(swapped.kind());
        swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
    }

    let ns = world.entities.init(id.kind());

    *ns.get_mut(id).expect("Entity is not valid") = loc;
}

/// The main entry point of the ECS
///
/// Holds the entities and components of the ECS.
///
/// Provides methods to spawn, despawn and access entities and components.
///
/// More advanced methods for component accesses are provided through the [`EntityRef`] and [`EntityRefMut`] acquired through [`Self::entity`] and
/// [`Self::entity_mut`], respectively.
///
/// For efficient iteration, change tracking, and graph traversal, see [`Query`]
pub struct World {
    entities: EntityStores,
    pub(crate) archetypes: Archetypes,
    change_tick: AtomicU32,

    has_reserved: AtomicBool,
}

impl World {
    /// Creates a new empty world
    pub fn new() -> Self {
        Self {
            entities: EntityStores::new(),
            archetypes: Archetypes::new(),
            change_tick: AtomicU32::new(0b11),
            has_reserved: AtomicBool::new(false),
        }
    }

    /// Reserve a single entity id concurrently.
    ///
    /// See: [`World::reserve`]
    pub fn reserve_one(&self, kind: EntityKind) -> Entity {
        self.has_reserved.store(true, Relaxed);
        self.entities.get(kind).unwrap().reserve_one()
    }

    /// Reserve entities id concurrently.
    ///
    /// The returned entity ids can be used directly by functions such as [ `set` ]( World::set ) and
    /// [ `spawn_at` ]( World::spawn_at ), but will not be yielded by queries until properly spawned by
    /// by adding a component or using spawn_at.
    pub fn reserve(&self, kind: EntityKind, count: usize) -> ReservedEntityIter {
        self.has_reserved.store(true, Relaxed);
        let iter = self.entities.get(kind).unwrap().reserve(count);
        ReservedEntityIter(iter)
    }

    /// Create an iterator to spawn several entities
    pub fn spawn_many(&mut self) -> impl Iterator<Item = Entity> + '_ {
        profile_function!();
        (0..).map(|_| self.spawn())
    }

    /// Spawn a new empty entity into the default namespace
    pub fn spawn(&mut self) -> Entity {
        profile_function!();
        self.spawn_inner(self.archetypes.root, EntityKind::empty())
            .0
    }

    /// Spawn a new empty entity and acquire an entity reference.
    pub fn spawn_ref(&mut self) -> EntityRefMut {
        profile_function!();
        let (id, loc, _) = self.spawn_inner(self.archetypes.root, EntityKind::empty());
        EntityRefMut {
            world: self,
            loc: OnceCell::with_value(loc),
            id,
        }
    }

    /// Efficiently spawn many entities with the same components at once.
    pub fn spawn_batch(&mut self, chunk: &mut BatchSpawn) -> Vec<Entity> {
        profile_function!();
        self.flush_reserved();

        for component in chunk.components() {
            self.init_component(component);
        }

        let change_tick = self.advance_change_tick();

        let (arch_id, arch) = self.archetypes.find_create(chunk.components());

        let base = arch.len();
        let store = self.entities.init(EntityKind::empty());

        let ids = (0..chunk.len())
            .map(|idx| {
                store.spawn(EntityLocation {
                    slot: base + idx,
                    arch_id,
                })
            })
            .collect_vec();

        let _ = arch.allocate_n(&ids);

        for (_, mut storage) in chunk.take_all() {
            unsafe {
                arch.extend(&mut storage, change_tick);
            }
        }

        ids
    }

    // Check if the entity is reserved after flush
    fn is_reserved(&self, id: Entity) -> bool {
        self.location(id)
            .map(|v| v.arch_id == self.archetypes.reserved)
            .unwrap_or_default()
    }

    /// Spawns an entitiy with a specific id.
    /// Fails if an entity with the same index already exists.
    pub fn spawn_at(&mut self, id: Entity) -> Result<Entity> {
        self.spawn_at_inner(id, self.archetypes.root)?;
        Ok(id)
    }

    /// Spawns an entitiy with a specific id.
    fn spawn_at_inner(
        &mut self,
        id: Entity,
        arch_id: ArchetypeId,
    ) -> Result<(EntityLocation, &mut Archetype)> {
        self.flush_reserved();

        if self.is_reserved(id) {
            self.despawn(id).unwrap();
        }

        let store = self.entities.init(id.kind());
        let arch = self.archetypes.get_mut(arch_id);

        let loc = store.spawn_at(id.index, id.gen, EntityLocation { slot: 0, arch_id })?;

        loc.slot = arch.allocate(id);

        Ok((*loc, arch))
    }

    pub(crate) fn spawn_at_with(
        &mut self,
        id: Entity,
        buffer: &mut ComponentBuffer,
    ) -> Result<(Entity, EntityLocation)> {
        let change_tick = self.advance_change_tick();

        for &component in buffer.components() {
            self.init_component(component);
        }

        let (arch_id, _) = self.archetypes.find_create(buffer.components().copied());
        let (loc, arch) = self.spawn_at_inner(id, arch_id)?;

        for (desc, src) in buffer.drain() {
            unsafe { arch.push(desc.key(), src, change_tick) }
        }

        Ok((id, loc))
    }

    /// Spawn an entity with the given components.
    ///
    /// For increased ergonomics, prefer [crate::EntityBuilder]
    pub(crate) fn spawn_with(&mut self, buffer: &mut ComponentBuffer) -> Entity {
        for component in buffer.components() {
            self.init_component(*component);
        }

        let change_tick = self.advance_change_tick();
        let (arch_id, _) = self.archetypes.find_create(buffer.components().copied());

        let (id, _, arch) = self.spawn_inner(arch_id, EntityKind::empty());

        for (desc, src) in buffer.drain() {
            unsafe {
                arch.push(desc.key, src, change_tick);
            }
        }

        id
    }

    /// Removes all components from an entity without despawning the entity
    pub fn clear(&mut self, id: Entity) -> Result<()> {
        let EntityLocation { arch_id, slot } = self.init_location(id)?;

        let (src, dst) = self
            .archetypes
            .get_disjoint(arch_id, self.archetypes.root)
            .unwrap();

        let (dst_slot, swapped) = unsafe { src.move_to(dst, slot, |c, p| c.drop(p)) };

        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            self.entities
                .init(swapped.kind())
                .get_mut(swapped)
                .expect("Invalid entity id")
                .slot = slot;
        }

        // self.archetypes.prune_arch(arch_id);

        *self.location_mut(id).unwrap() = EntityLocation {
            slot: dst_slot,
            arch_id: self.archetypes.root,
        };

        Ok(())
    }

    /// Prune empty archetypes, returning the number of archetypes removed
    pub fn prune_archetypes(&mut self) -> usize {
        self.archetypes.prune_all()
    }

    pub(crate) fn retain_entity_components(
        &mut self,
        id: Entity,
        loc: EntityLocation,
        mut f: impl FnMut(ComponentKey) -> bool,
    ) -> EntityLocation {
        let src = self.archetypes.get(loc.arch_id);

        let dst_components: SmallVec<[ComponentDesc; 8]> =
            src.components_desc().filter(|v| f(v.key())).collect();

        let (dst_id, _) = self.archetypes.find_create(dst_components);

        let (src, dst) = self.archetypes.get_disjoint(loc.arch_id, dst_id).unwrap();

        let (dst_slot, swapped) = unsafe { src.move_to(dst, loc.slot, |c, p| c.drop(p)) };

        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            self.entities
                .init(swapped.kind())
                .get_mut(swapped)
                .expect("Invalid entity id")
                .slot = slot;
        }

        // self.archetypes.prune_arch(loc.arch_id);
        let loc = EntityLocation {
            slot: dst_slot,
            arch_id: dst_id,
        };

        *self.location_mut(id).expect("Entity is not valid") = loc;
        loc
    }

    /// Set metadata for a given component if they do not already exist
    pub(crate) fn init_component(&mut self, desc: ComponentDesc) {
        assert!(
            desc.key().id.kind().contains(EntityKind::COMPONENT),
            "Component is not a component kind id"
        );

        if self.is_alive(desc.key.id()) {
            return;
        }

        let id = desc.key().id;
        let mut meta = desc.create_meta();
        meta.set(component_info(), desc);
        meta.set(name(), desc.name().into());

        if id.is_static() {
            meta.set(is_static_entity(), ());
        }
        self.spawn_at(id).unwrap();

        self.set_with(id, &mut meta).unwrap();
    }

    /// Despawn an entity.
    /// Any relations to other entities will be removed.
    pub fn despawn(&mut self, id: Entity) -> Result<()> {
        profile_function!();
        self.flush_reserved();
        let EntityLocation {
            arch_id: arch,
            slot,
        } = self.init_location(id)?;

        // if id.is_static() {
        //     panic!("Attempt to despawn static component");
        // }

        let src = self.archetypes.get_mut(arch);

        let swapped = unsafe {
            src.take(slot, |c, p| {
                c.drop(p);
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

        // self.archetypes.prune_arch(arch);
        self.entities.init(id.kind()).despawn(id)?;
        self.detach(id);
        Ok(())
    }

    /// Despawns all entities which matches the filter
    pub fn despawn_many<F>(&mut self, filter: F)
    where
        F: for<'x> Fetch<'x>,
    {
        profile_function!();
        self.flush_reserved();
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
        profile_function!();
        self.despawn_children(id, relation)?;
        self.despawn(id)?;

        Ok(())
    }

    /// Despawns all children of an entity recursively
    pub fn despawn_children<T: ComponentValue>(
        &mut self,
        id: Entity,
        relation: impl RelationExt<T>,
    ) -> Result<()> {
        profile_function!();
        self.flush_reserved();

        let mut stack = alloc::vec![id];
        let mut archetypes = Vec::new();
        while let Some(id) = stack.pop() {
            profile_scope!("traverse_archetypes");
            archetypes.clear();
            archetypes.extend(
                self.archetypes
                    .index
                    .find(relation.of(id).key())
                    .into_iter()
                    .flat_map(|v| v.keys().copied()),
            );

            for &arch_id in &archetypes {
                let arch = self.archetypes.get(arch_id);
                stack.extend(arch.entities());
                for &id in arch.entities() {
                    self.entities.init(id.kind()).despawn(id).unwrap();
                }
                self.archetypes.despawn(arch_id).clear();
            }
        }

        Ok(())
    }

    /// Removes all instances of relations and component of the given entities
    /// in the world. If used upon an entity with a child -> parent relation, this removes the relation
    /// on all the children.
    pub fn detach(&mut self, id: Entity) {
        profile_function!();
        let index = &self.archetypes.index;
        let archetypes = index
            .find_relation_targets(id)
            .into_iter()
            .chain(index.find_relation(id))
            .chain(index.find(ComponentKey::new(id, None)))
            .flat_map(|v| v.keys().copied())
            .collect_vec();

        for src in archetypes.into_iter().rev() {
            let mut src = self.archetypes.despawn(src);

            let components = src.components_desc().filter(|v| {
                let key = v.key();
                !(key.id == id || key.target == Some(id))
            });

            let (dst_id, dst) = self.archetypes.find_create(components);

            for (id, slot) in src.move_all(dst) {
                *self.location_mut(id).expect("Entity id was not valid") = EntityLocation {
                    slot,
                    arch_id: dst_id,
                }
            }
        }
    }

    /// Updates a component in place
    pub fn update<T: ComponentValue, U>(
        &self,
        id: Entity,
        component: Component<T>,
        f: impl FnOnce(&mut T) -> U,
    ) -> Result<U> {
        let change_tick = self.advance_change_tick();

        let EntityLocation {
            arch_id: src_id,
            slot,
        } = self.location(id)?;

        self.archetypes
            .get(src_id)
            .update(slot, component, FnWriter::new(f), change_tick)
            .ok_or(Error::MissingComponent(MissingComponent {
                id,
                desc: component.desc(),
            }))
    }

    /// Updates a component in place
    pub fn update_dedup<T: ComponentValue + PartialEq>(
        &self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> Result<()> {
        let tick = self.advance_change_tick();

        let EntityLocation {
            arch_id: src_id,
            slot,
        } = self.location(id)?;

        self.archetypes
            .get(src_id)
            .update(slot, component, WriteDedup::new(value), tick)
            .ok_or(Error::MissingComponent(MissingComponent {
                id,
                desc: component.desc(),
            }))
    }

    /// Set the value of a component.
    /// If the component does not exist it will be added.
    #[inline]
    pub fn set<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> Result<Option<T>> {
        Ok(self
            .set_with_writer(
                id,
                SingleComponentWriter::new(component.desc(), Replace::new(value)),
            )?
            .1
            .left())
    }

    /// Add the components stored in a component buffer to an entity
    pub fn set_with(&mut self, id: Entity, buffer: &mut ComponentBuffer) -> Result<()> {
        self.set_with_writer(id, writer::Buffered::new(buffer))?;

        Ok(())
    }

    #[inline]
    pub(crate) fn set_dyn(
        &mut self,
        id: Entity,
        desc: ComponentDesc,
        value: *mut u8,
    ) -> Result<EntityLocation> {
        let (loc, _) =
            self.set_with_writer(id, SingleComponentWriter::new(desc, ReplaceDyn { value }))?;

        Ok(loc)
    }

    #[inline]
    pub(crate) fn set_with_writer<U: EntityWriter>(
        &mut self,
        id: Entity,
        writer: U,
    ) -> Result<(EntityLocation, U::Output)> {
        // We know things will change either way
        let change_tick = self.advance_change_tick();

        let src_loc = self.init_location(id)?;

        Ok(writer.write(self, id, src_loc, change_tick))
    }

    #[inline]
    pub(crate) fn remove_dyn(&mut self, id: Entity, component: ComponentDesc) -> Result<()> {
        unsafe {
            self.remove_inner(id, component, |ptr| component.drop(ptr))
                .map(|_| {})
        }
    }

    pub(crate) unsafe fn remove_inner(
        &mut self,
        id: Entity,
        desc: ComponentDesc,
        on_drop: impl FnOnce(*mut u8),
    ) -> Result<EntityLocation> {
        let EntityLocation {
            arch_id: src_id,
            slot,
        } = self.init_location(id).unwrap();

        let src = self.archetypes.get(src_id);

        if !src.has(desc.key()) {
            return Err(Error::MissingComponent(MissingComponent { id, desc }));
        }

        let dst_id = match src.incoming(desc.key()) {
            Some(dst) => dst,
            None => {
                let components = src
                    .components_desc()
                    .filter(|v| v.key != desc.key())
                    .collect_vec();

                let (dst_id, _) = self.archetypes.find_create(components);

                dst_id
            }
        };

        assert_ne!(src_id, dst_id);
        // Borrow disjoint
        let (src, dst) = self.archetypes.get_disjoint(src_id, dst_id).unwrap();
        src.add_incoming(desc.key(), dst_id);
        dst.add_outgoing(desc.key(), src_id);

        // Take the value
        // This moves the differing value out of the archetype before it is
        // forgotten in the move

        // Capture the ONE moved value
        let mut on_drop = Some(on_drop);
        let (dst_slot, swapped) = src.move_to(dst, slot, |_, p| {
            let drop = on_drop.take().expect("On drop called more than once");
            (drop)(p);
        });

        debug_assert_eq!(dst.entity(dst_slot), Some(id));

        if let Some((swapped, slot)) = swapped {
            // The last entity in src was moved into the slot occupied by id
            let swapped_ns = self.entities.init(swapped.kind());
            swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        let loc = EntityLocation {
            slot: dst_slot,
            arch_id: dst_id,
        };

        *self.location_mut(id).expect("Entity is not valid") = loc;

        Ok(loc)
    }

    /// Remove a component from the entity
    #[inline]
    pub fn remove<T: ComponentValue>(&mut self, id: Entity, component: Component<T>) -> Result<T> {
        let mut res: MaybeUninit<T> = MaybeUninit::uninit();
        let res = unsafe {
            self.remove_inner(id, component.desc(), |ptr| {
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

        self.get_at(loc, component).ok_or_else(|| {
            Error::MissingComponent(MissingComponent {
                id,
                desc: component.desc(),
            })
        })
    }

    #[inline]
    pub(crate) fn get_at<T: ComponentValue>(
        &self,
        EntityLocation {
            arch_id: arch,
            slot,
        }: EntityLocation,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        self.archetypes.get(arch).get(slot, component)
    }

    pub(crate) fn try_get_at<T: ComponentValue>(
        &self,
        EntityLocation {
            arch_id: arch,
            slot,
        }: EntityLocation,
        component: Component<T>,
    ) -> core::result::Result<Option<AtomicRef<T>>, BorrowError> {
        self.archetypes.get(arch).try_get(slot, component)
    }

    /// Randomly access an entity's component.
    pub fn get_mut<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Result<RefMut<T>> {
        let loc = self.location(id)?;

        self.get_mut_at(loc, component).ok_or_else(|| {
            Error::MissingComponent(MissingComponent {
                id,
                desc: component.desc(),
            })
        })
    }

    /// Randomly access an entity's component.
    pub(crate) fn get_mut_at<T: ComponentValue>(
        &self,
        EntityLocation {
            arch_id: arch,
            slot,
        }: EntityLocation,
        component: Component<T>,
    ) -> Option<RefMut<T>> {
        self.archetypes
            .get(arch)
            .get_mut(slot, component, self.advance_change_tick())
    }

    /// Randomly access an entity's component.
    pub(crate) fn try_get_mut_at<T: ComponentValue>(
        &self,
        EntityLocation {
            arch_id: arch,
            slot,
        }: EntityLocation,
        component: Component<T>,
    ) -> core::result::Result<Option<RefMut<T>>, BorrowMutError> {
        self.archetypes
            .get(arch)
            .try_get_mut(slot, component, self.advance_change_tick())
    }

    /// Returns true if the entity has the specified component.
    /// Returns false if the entity does not exist or it does not have the
    /// specified component
    pub fn has<T: ComponentValue>(&self, id: Entity, component: Component<T>) -> bool {
        if let Ok(loc) = self.location(id) {
            self.archetypes.get(loc.arch_id).has(component.key())
        } else {
            false
        }
    }

    /// Returns true if the entity is still alive.
    ///
    /// **Note**: false is returned static entities which are not yet present in the world, for example, before
    /// inserting a first component.
    ///
    /// This is because static entities and components are lazily initialized on first insertion or
    /// other modification.
    pub fn is_alive(&self, id: Entity) -> bool {
        self.entities
            .get(id.kind())
            .map(|v| v.is_alive(id))
            .unwrap_or(false)
    }

    /// Returns the location inside an archetype for a given entity
    ///
    /// *Note*: Fails for static entities which are not yet spawned into the world, which happens
    /// when a component is first added.
    pub(crate) fn location(&self, id: Entity) -> Result<EntityLocation> {
        match self.entities.get(id.kind()).and_then(|v| v.get(id)) {
            Some(&loc) => Ok(loc),
            None => Err(Error::NoSuchEntity(id)),
        }
    }

    fn location_mut(&mut self, id: Entity) -> Result<&mut EntityLocation> {
        self.entities
            .init(id.kind())
            .get_mut(id)
            .ok_or(Error::NoSuchEntity(id))
    }

    /// Returns the entity location. If the entity is static it will first be spawned
    fn init_location(&mut self, id: Entity) -> Result<EntityLocation> {
        let store = self.entities.init(id.kind());

        match store.get(id) {
            Some(&loc) => Ok(loc),
            None if id.is_static() => self.ensure_static(id),
            None => {
                let mut found = Err(Error::NoSuchEntity(id));

                let reserved = self.archetypes.reserved;
                let arch = self.archetypes.get_mut(reserved);
                store.flush_reserved(|new_id| {
                    let slot = arch.allocate(new_id);

                    let loc = EntityLocation {
                        slot,
                        arch_id: reserved,
                    };

                    if new_id == id {
                        found = Ok(loc)
                    }
                    loc
                });

                found
            }
        }
    }

    /// Batch spawn multiple components with prespecified ids.
    /// Fails if any of the entities already exist.
    ///
    /// Returns the passed ids, to allow chaining with result.
    pub fn spawn_batch_at<'a>(
        &mut self,
        ids: &'a [Entity],
        chunk: &mut BatchSpawn,
    ) -> Result<&'a [Entity]> {
        for component in chunk.components() {
            self.init_component(component);
        }

        self.spawn_batch_at_inner(ids, chunk)
    }

    /// Does not initialize components
    fn spawn_batch_at_inner<'a>(
        &mut self,
        ids: &'a [Entity],
        chunk: &mut BatchSpawn,
    ) -> Result<&'a [Entity]> {
        self.flush_reserved();
        assert_eq!(
            ids.len(),
            chunk.len(),
            "The length of ids must match the number of slots in `batch`"
        );

        for &id in ids {
            if self.is_reserved(id) {
                self.despawn(id).unwrap();
            } else if let Some(v) = self.reconstruct(id.index(), id.kind()) {
                return Err(Error::EntityOccupied(v));
            }
        }

        let change_tick = self.advance_change_tick();

        let (arch_id, arch) = self.archetypes.find_create(chunk.components());

        let base = arch.len();
        for (idx, &id) in ids.iter().enumerate() {
            let kind = id.kind();
            let store = self.entities.init(kind);
            assert_eq!(store.kind, kind);
            store
                .spawn_at(
                    id.index(),
                    id.gen(),
                    EntityLocation {
                        slot: base + idx,
                        arch_id,
                    },
                )
                // The vacancy was checked prior
                .unwrap();
        }

        let _ = arch.allocate_n(ids);

        let arch = self.archetypes.get_mut(arch_id);

        for (_, mut storage) in chunk.take_all() {
            unsafe {
                arch.extend(&mut storage, change_tick);
            }
        }

        Ok(ids)
    }

    /// Spawn a new component of type `T` which can be attached to an entity.
    ///
    /// The given name does not need to be unique.
    pub fn spawn_component<T: ComponentValue>(
        &mut self,
        vtable: &'static ComponentVTable<T>,
    ) -> Component<T> {
        let (id, _, _) = self.spawn_inner(self.archetypes.root, EntityKind::COMPONENT);

        // Safety
        // The id is not used by anything else
        let component = Component::new(ComponentKey::new(id, None), vtable);

        let desc = component.desc();

        let mut meta = desc.create_meta();
        meta.set(component_info(), desc);
        meta.set(components::name(), desc.name().into());

        self.set_with(id, &mut meta).unwrap();
        component
    }

    /// Spawn a new relation of type `T` which can be attached to an entity.
    ///
    /// The given name does not need to be unique.
    pub fn spawn_relation<T: ComponentValue>(
        &mut self,
        vtable: &'static ComponentVTable<T>,
    ) -> Relation<T> {
        let (id, _, _) = self.spawn_inner(self.archetypes.root, EntityKind::COMPONENT);

        Relation::new(id, vtable)
    }

    #[inline(always)]
    fn spawn_inner(
        &mut self,
        arch_id: ArchetypeId,
        kind: EntityKind,
    ) -> (Entity, EntityLocation, &mut Archetype) {
        self.flush_reserved();
        // Place at root
        let ns = self.entities.init(kind);

        let arch = self.archetypes.get_mut(arch_id);

        let slot = arch.len();

        let loc = EntityLocation { arch_id, slot };

        let id = ns.spawn(loc);

        arch.allocate(id);

        (id, loc, arch)
    }

    /// Get a reference to the world's archetype generation
    #[must_use]
    pub fn archetype_gen(&self) -> u32 {
        self.archetypes.gen()
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
    pub fn format_entities<'a>(&'a self, ids: &'a [Entity]) -> EntitiesFormatter<'a> {
        EntitiesFormatter { world: self, ids }
    }

    /// Formats a hierarchy entites formed by the specified relation
    pub fn format_hierarchy<T: ComponentValue>(
        &self,
        relation: impl RelationExt<T>,
        id: Entity,
    ) -> HierarchyFormatter<'_> {
        let loc = self.location(id).expect("Invalid entity");
        let arch = self.archetypes.get(loc.arch_id);
        let relation = relation.id();

        HierarchyFormatter {
            world: self,
            id,
            slot: loc.slot,
            arch,
            relation,
        }
    }

    /// Returns a human friendly breakdown of the archetypes in the world
    pub fn archetype_info(&self) -> BTreeMap<ArchetypeId, ArchetypeInfo> {
        self.archetypes.iter().map(|(k, v)| (k, v.desc())).collect()
    }

    /// Attempt to find an alive entity given the id
    pub fn reconstruct(&self, index: EntityIndex, kind: EntityKind) -> Option<Entity> {
        let ns = self.entities.get(kind)?;

        ns.reconstruct(index).map(|v| v.0)
    }

    /// Attempt to find a component from the given id
    pub fn find_component<T: ComponentValue>(&self, id: ComponentKey) -> Option<Component<T>> {
        let e = self.entity(id.id).ok()?;

        let desc = e.get(component_info()).ok()?;

        if !desc.is::<T>() {
            panic!("Attempt to construct a component from the wrong type. Found: {desc:#?}");
        }
        // Safety: the type

        Some(Component::from_raw_parts(id, desc.vtable))
    }

    /// Access, insert, and remove all components of an entity
    pub fn entity_mut(&mut self, id: Entity) -> Result<EntityRefMut> {
        let loc = self.init_location(id)?;
        Ok(EntityRefMut {
            world: self,
            loc: OnceCell::with_value(loc),
            id,
        })
    }

    /// Access all components of an entity
    ///
    /// **Note**: Fails for static entities if they have not yet been spawned into the world
    pub fn entity(&self, id: Entity) -> Result<EntityRef> {
        let loc = self.location(id)?;
        let arch = self.archetypes.get(loc.arch_id);

        Ok(EntityRef {
            world: self,
            arch,
            loc,
            id,
        })
    }

    /// Returns an entry for a given component of an entity allowing for
    /// in-place manipulation, insertion or removal.
    ///
    /// Fails if the entity is not alive.
    pub fn entry<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
    ) -> Result<Entry<T>> {
        let loc = self.init_location(id)?;
        let arch = self.archetypes.get(loc.arch_id);
        if arch.has(component.key()) {
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

    /// Subscribe to events in the world using the provided event handler.
    ///
    /// This allows reacting to changes in systems, and in async contexts by using channels or [`tokio::sync::Notify`].
    pub fn subscribe<S>(&mut self, subscriber: S)
    where
        S: EventSubscriber,
    {
        self.archetypes.add_subscriber(Arc::new(subscriber))
    }

    /// Merges `other` into `self`.
    ///
    /// Colliding entities will be migrated to a new entity id. Static entities will not be
    /// migrated but rather appended to existing ones. This is so that e.g; a resource entity gets
    /// the union of the worlds.
    ///
    /// Returns a map of all the entities which were remapped.
    ///
    /// `other` will be left empty
    ///
    /// **Note**: The data from `other` will all be marked as *added*
    /// as change events do not carry over.
    pub fn merge_with(&mut self, other: &mut World) -> MigratedEntities {
        let mut archetypes = mem::replace(&mut other.archetypes, Archetypes::new());
        let mut entities = mem::take(&mut other.entities);

        let mut components = BTreeMap::new();

        self.flush_reserved();

        let mut new_ids = BTreeMap::new();

        let mut buffer = Entity::builder();

        for (arch_id, arch) in archetypes.iter_mut() {
            if !arch.has(is_static_entity().key()) {
                for id in arch.entities_mut() {
                    let old_id = *id;
                    let loc = entities
                        .init(id.kind())
                        .get(*id)
                        .expect("Invalid component");

                    debug_assert_eq!(loc.arch_id, arch_id);
                    // Migrate
                    if self.reconstruct(old_id.index, old_id.kind).is_some() {
                        let new_id = self.reserve_one(old_id.kind);
                        self.flush_reserved();

                        // Change the id inside of the archetype
                        *id = new_id;
                        new_ids.insert(old_id, new_id);
                    } else {
                        // Make sure nothing is spawned here in the meantime
                        self.reserve_at(old_id).unwrap();
                    }
                }

                if arch.has(component_info().key()) {
                    // Make sure to reinsert any non-static components
                    for (slot, &id) in arch.slots().iter().zip(arch.entities()) {
                        components
                            .insert(id, *arch.get(slot, component_info()).expect("Invalid slot"));
                    }
                }
            }
        }

        for (_, arch) in archetypes.iter_mut() {
            // Don't migrate static components
            if !arch.has(is_static_entity().key()) {
                let mut batch = BatchSpawn::new(arch.len());
                let arch = arch.drain();
                for mut cell in arch.cells.into_vec().into_iter() {
                    let mut storage = cell.drain();
                    let mut id = storage.desc().key;

                    // Modify the relations to match new components
                    id.id = *new_ids.get(&id.id).unwrap_or(&id.id);

                    if let Some(ref mut target) = id.target {
                        *target = *new_ids.get(target).unwrap_or(target);
                    }

                    // Safety
                    // The component is still of the same type
                    unsafe {
                        storage.set_id(id);
                    }

                    batch.append(storage).expect("Batch is incomplete");
                }

                // Skip initializing components as component entities will be added by further
                // iterations of the loop, and can thus not be spawned as they need to be
                // unoccupied.
                self.spawn_batch_at_inner(&arch.entities, &mut batch)
                    .expect("Failed to spawn batch");
            }
        }

        // Append all static ids
        // This happens after non-static components have been initialized
        for (_, arch) in archetypes.iter_mut() {
            // Take each entity one by one and append them to the world
            if arch.has(is_static_entity().key()) {
                while let Some(id) = unsafe {
                    arch.pop_last(|mut desc, ptr| {
                        let key = &mut desc.key;

                        // Modify the relations to match new components
                        key.id = *new_ids.get(&key.id).unwrap_or(&key.id);

                        if let Some(ref mut target) = key.target {
                            *target = *new_ids.get(target).unwrap_or(target);
                        }

                        // Migrate custom components
                        buffer.set_dyn(desc, ptr);
                    })
                } {
                    buffer.append_to(self, id).unwrap();
                }
            }
        }
        MigratedEntities { ids: new_ids }
    }

    /// Converts all reserved entity ids into actual empty entities placed in a special archetype.
    #[inline]
    fn flush_reserved(&mut self) {
        if !self.has_reserved.swap(false, Relaxed) {
            return;
        }

        let reserved = self.archetypes.reserved;
        let arch = self.archetypes.get_mut(reserved);

        for store in self.entities.inner.values_mut() {
            store.flush_reserved(|id| {
                let slot = arch.allocate(id);

                EntityLocation {
                    slot,
                    arch_id: reserved,
                }
            })
        }
    }

    fn reserve_at(&mut self, id: Entity) -> Result<()> {
        self.flush_reserved();
        self.entities.init(id.kind).reserve_at(id.index())
    }

    /// Ensure a static entity id exists
    fn ensure_static(&mut self, id: Entity) -> Result<EntityLocation> {
        assert!(id.is_static());
        let mut buffer = ComponentBuffer::new();
        buffer.set(is_static_entity(), ());
        let (_, loc) = self.spawn_at_with(id, &mut buffer)?;
        Ok(loc)
    }
}

/// Holds the migrated components
#[derive(Debug, Clone)]
pub struct MigratedEntities {
    ids: BTreeMap<Entity, Entity>,
}

impl MigratedEntities {
    /// Retuns the new id if it was migrated, otherwise, returns the given id
    pub fn get(&self, id: Entity) -> Entity {
        *self.ids.get(&id).unwrap_or(&id)
    }

    /// Returns the migrated component. All components are migrated
    /// # Panics
    /// If the types do not match
    pub fn get_component<T: ComponentValue>(&self, component: Component<T>) -> Component<T> {
        let id = self.get(component.key().id);
        let target = component.key().target.map(|v| self.get(v));

        Component::from_raw_parts(ComponentKey::new(id, target), component.vtable)
    }

    /// Returns the migrated relation
    /// # Panics
    /// If the types do not match
    pub fn get_relation<T: ComponentValue>(
        &self,
        relation: impl RelationExt<T>,
    ) -> impl Fn(Entity) -> Component<T> {
        let component = relation.of(dummy());

        let component = self.get_component(component);

        move |target| component.of(target)
    }

    /// Returns the migrated ids
    pub fn ids(&self) -> &BTreeMap<Entity, Entity> {
        &self.ids
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.format_debug(component_info().without()).fmt(f)
    }
}

/// Iterates reserved entity ids.
///
/// See: [`World::reserve`]
pub struct ReservedEntityIter<'a>(crate::entity::ReservedIter<'a>);

impl<'a> ExactSizeIterator for ReservedEntityIter<'a> {
    fn len(&self) -> usize {
        self.0.len()
    }
}

impl<'a> Iterator for ReservedEntityIter<'a> {
    type Item = Entity;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

#[cfg(test)]
mod tests {

    use core::iter::repeat;

    use alloc::{string::String, sync::Arc};

    use crate::{component, CommandBuffer, EntityBuilder, FetchExt, Query};

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
        let (_, archetype) = world
            .archetypes
            .find_create([a().desc(), b().desc(), c().desc()]);
        assert!(!archetype.has(d().key()));
        assert!(archetype.has(a().key()));
        assert!(archetype.has(b().key()));

        // () -> (a) -> (ab) -> (abc)
        //                   -> (abd)
        let (_, archetype) = world
            .archetypes
            .find_create([a().desc(), b().desc(), d().desc()]);
        assert!(archetype.has(d().key()));
        assert!(!archetype.has(c().key()));
    }

    #[test]
    fn insert() {
        let mut world = World::new();
        let id = world.spawn();

        world.set(id, a(), 65).unwrap();
        let shared: Arc<String> = Arc::new("Foo".into());

        assert_eq!(world.get(id, a()).as_deref(), Ok(&65));
        assert_eq!(
            world.get(id, b()).as_deref(),
            Err(&Error::MissingComponent(MissingComponent {
                id,
                desc: b().desc()
            }))
        );
        assert!(!world.has(id, c()));

        let id2 = world.spawn();
        world.set(id2, a(), 7).unwrap();

        world.set(id2, c(), "Foo".into()).unwrap();

        // eprintln!("a: {}, b: {}, c: {}, id: {}", a(), a(), c(), id);

        assert_eq!(world.get(id, a()).as_deref(), Ok(&65));
        assert_eq!(
            world.get(id, b()).as_deref(),
            Err(&Error::MissingComponent(MissingComponent {
                id,
                desc: b().desc()
            }))
        );

        assert!(!world.has(id, c()));

        assert_eq!(world.get(id2, a()).as_deref(), Ok(&7));
        assert_eq!(world.get(id2, c()).as_deref(), Ok(&"Foo".into()));
        world.set(id, e(), shared.clone()).unwrap();
        assert_eq!(
            world.get(id, e()).as_deref().map(|v| &**v),
            Ok(&"Foo".into())
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
            .set(c(), "Foo".into())
            .spawn(&mut world);

        let shared: Arc<String> = Arc::new("The meaning of life is ...".into());

        world.set(id, e(), shared.clone()).unwrap();
        let id2 = EntityBuilder::new()
            .set(a(), 6)
            .set(b(), 0.219)
            .set(c(), "Bar".into())
            .set(e(), shared.clone())
            .spawn(&mut world);

        assert_eq!(world.get(id, b()).as_deref(), Ok(&0.3));
        assert_eq!(world.get(id, e()).as_deref(), Ok(&shared));

        assert_eq!(world.remove(id, e()).as_ref(), Ok(&shared));

        assert_eq!(world.get(id, a()).as_deref(), Ok(&9));
        assert_eq!(world.get(id, c()).as_deref(), Ok(&"Foo".into()));
        assert_eq!(
            world.get(id, e()).as_deref(),
            Err(&Error::MissingComponent(MissingComponent {
                id,
                desc: e().desc()
            }))
        );

        world.despawn(id).unwrap();

        assert_eq!(world.get(id, a()).as_deref(), Err(&Error::NoSuchEntity(id)));
        assert_eq!(world.get(id, c()).as_deref(), Err(&Error::NoSuchEntity(id)));
        assert_eq!(world.get(id, e()).as_deref(), Err(&Error::NoSuchEntity(id)));

        assert_eq!(world.get(id2, e()).as_deref(), Ok(&shared));
        assert_eq!(world.get(id2, c()).as_deref(), Ok(&"Bar".into()));

        assert_eq!(world.get(id, e()).as_deref(), Err(&Error::NoSuchEntity(id)));

        assert_eq!(Arc::strong_count(&shared), 2);

        // // Remove id

        let mut query = Query::new((a().cloned(), c().cloned()));
        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, [(6, "Bar".into())]);
    }

    #[test]
    fn reserve() {
        let mut world = World::new();

        let a = world.spawn();

        let b = world.reserve_one(Default::default());

        let c = world.spawn();
        let short_lived = world.spawn();
        world.despawn(short_lived).unwrap();

        world.set(b, name(), "b".into()).unwrap();
        world.set(a, name(), "a".into()).unwrap();
        world.set(c, name(), "c".into()).unwrap();

        let reserved = world.reserve(Default::default(), 4).collect_vec();

        let mut cmd = CommandBuffer::new();
        cmd.spawn_batch_at(
            reserved.clone(),
            BatchSpawn::new(4)
                .set(name(), repeat("I am one and the same".into()))
                .unwrap(),
        );

        cmd.apply(&mut world).unwrap();

        let items: Vec<(Entity, String)> = Query::new((entity_ids(), name()))
            .borrow(&world)
            .iter()
            .map(|(id, name)| (id, name.into()))
            .sorted()
            .collect_vec();

        assert_eq!(
            items,
            [(a, "a".into()), (b, "b".into()), (c, "c".into())]
                .into_iter()
                .chain(
                    reserved
                        .into_iter()
                        .zip(repeat("I am one and the same".into()))
                )
                .collect_vec()
        );
    }

    #[test]
    fn reserve_set() {
        let mut world = World::new();

        let a = world.spawn();

        let b = world.reserve_one(Default::default());

        world.set(b, name(), "b".into()).unwrap();
        world.set(a, name(), "a".into()).unwrap();

        let reserved = world.reserve(Default::default(), 4).collect_vec();

        let mut cmd = CommandBuffer::new();
        cmd.spawn_batch_at(
            reserved.clone(),
            BatchSpawn::new(4)
                .set(name(), repeat("I am one and the same".into()))
                .unwrap(),
        );

        cmd.apply(&mut world).unwrap();

        let items: Vec<(Entity, String)> = Query::new((entity_ids(), name()))
            .borrow(&world)
            .iter()
            .map(|(id, name)| (id, name.into()))
            .sorted()
            .collect_vec();

        assert_eq!(
            items,
            [(a, "a".into()), (b, "b".into())]
                .into_iter()
                .chain(
                    reserved
                        .into_iter()
                        .zip(repeat("I am one and the same".into()))
                )
                .collect_vec()
        );
    }
}
