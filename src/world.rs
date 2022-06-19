use std::{
    collections::BTreeMap,
    mem::{self, MaybeUninit},
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use itertools::Itertools;

use crate::{
    archetype::{Archetype, ArchetypeId, Change, ComponentInfo, Slice, Visitor},
    entity::{EntityLocation, EntityStore},
    error::Result,
    Component, ComponentBuffer, ComponentId, ComponentValue, Entity, Error, Namespace,
    STATIC_NAMESPACE,
};

pub struct World {
    entities: BTreeMap<Namespace, EntityStore>,
    archetypes: EntityStore<Archetype>,
    archetype_root: ArchetypeId,
    change_tick: AtomicU32,
    archetype_gen: AtomicU32,
}

impl World {
    pub fn new() -> Self {
        let mut archetypes = EntityStore::new(255);
        let root = archetypes.spawn(Archetype::empty());

        Self {
            entities: BTreeMap::new(),
            archetypes,
            change_tick: AtomicU32::new(0),
            archetype_gen: AtomicU32::new(0),
            archetype_root: root,
        }
    }

    pub fn get_namespace(&self, namespace: Namespace) -> Result<&EntityStore> {
        self.entities
            .get(&namespace)
            .ok_or(Error::NoSuchNamespace(namespace))
    }

    pub fn init_namespace(&mut self, namespace: Namespace) -> &mut EntityStore {
        self.entities
            .entry(namespace)
            .or_insert_with(|| EntityStore::new(namespace))
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
        root: ArchetypeId,
        components: impl IntoIterator<Item = &'a ComponentInfo>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut components = components.into_iter();
        let mut cursor = root;

        while let Some(head) = components.next() {
            let cur = &mut self.archetypes.get(cursor).unwrap();
            cursor = match cur.edge_to(head.id) {
                Some(id) => id,
                None => {
                    let new = Archetype::new(cur.components().copied().chain([*head]));
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
        self.spawn_in(0)
    }

    pub fn spawn_in(&mut self, namespace: Namespace) -> Entity {
        // Place at root
        let arch = self.archetype_root;
        let ns = self.init_namespace(namespace);
        let id = ns.spawn(EntityLocation { arch, slot: 0 });

        let slot = self.archetype_mut(self.archetype_root).allocate(id);

        // This is safe as `root` does not contain any components
        self.init_namespace(namespace).get_mut(id).unwrap().slot = slot;
        id
    }

    pub fn spawn_at(&mut self, id: Entity) -> EntityLocation {
        let namespace = id.namespace();
        let ns = self.init_namespace(namespace);

        if let Some(location) = ns.get(id) {
            *location
        } else {
            let root = self.archetype_root;
            let slot = self.archetype_mut(root).allocate(id);
            let location = EntityLocation { arch: root, slot };

            *self.init_namespace(namespace).spawn_at(id, location)
        }
    }

    /// Access an archetype by id
    pub fn archetype(&self, id: ArchetypeId) -> &Archetype {
        &self.archetypes.get(id).unwrap()
    }

    /// Access an archetype by id
    pub fn archetype_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        self.archetypes.get_mut(id).unwrap()
    }

    /// Spawn an entity with the given components.
    ///
    /// For increased ergonomics, prefer [crate::EntityBuilder]
    pub fn spawn_with(&mut self, namespace: Namespace, components: &mut ComponentBuffer) -> Entity {
        let id = self.spawn_in(namespace);

        let change_tick = self.advance_change_tick();

        let (archetype_id, arch) =
            self.fetch_archetype(self.archetype_root, components.components());

        let slot = arch.allocate(id);

        for component in components.components() {
            arch.init_changes(component.id)
                .set(Change::inserted(Slice::single(slot), change_tick));
        }

        unsafe {
            for (component, src) in components.take_all() {
                let storage = arch.storage_raw(component.id).unwrap();
                std::ptr::copy_nonoverlapping(src, storage.at(slot), component.size());
            }
        }

        *self.init_namespace(namespace).get_mut(id).unwrap() = EntityLocation {
            arch: archetype_id,
            slot,
        };

        id
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

        let EntityLocation { arch, slot } = self.init_location(id)?;

        let mut new_data = Vec::new();
        let mut new_components = Vec::new();

        let src_id = arch;
        let src = self.archetype_mut(arch);
        for (component, data) in components {
            if let Some(old) = src.get_dyn(slot, component.id) {
                // Drop old
                (component.drop)(old);
                ptr::copy_nonoverlapping(data, old, component.size());

                eprintln!("Replacing {component:?}");

                src.changes_mut(component.id())
                    .unwrap()
                    .set(Change::modified(Slice::single(slot), change_tick));
            } else {
                // Component does not exist yet, so defer a move

                // Data will have a lifetime of `components`.
                eprintln!("Deferring {component:?}");
                new_data.push((component, data));
                new_components.push(component);
            }
        }

        if !new_data.is_empty() {
            new_components.extend(src.components().copied());

            // Make sure everything is in its order
            new_components.sort_unstable();

            let components = new_components;

            let (dst_id, _) = self.fetch_archetype(self.archetype_root, components.iter());

            // Borrow disjoint
            let (src, dst) = self.archetypes.get_disjoint(src_id, dst_id).unwrap();

            let (dst_slot, swapped) =
                src.move_to(dst, slot, |c, _| panic!("Component {c:#?} was removed"));

            // Insert the missing components
            for (component, data) in new_data {
                dst.put_dyn(dst_slot, &component, data)
                    .expect("Insert should not fail");

                dst.init_changes(component.id())
                    .set(Change::inserted(Slice::single(dst_slot), change_tick));
            }

            assert_eq!(dst.entity(dst_slot), Some(id));

            // Migrate all changes
            src.migrate_slot(dst, slot, dst_slot);

            if let Some(swapped) = swapped {
                // The last entity in src was moved into the slot occupied by id
                eprintln!("Relocating entity");
                self.init_namespace(swapped.namespace())
                    .get_mut(swapped)
                    .expect("Invalid entity id")
                    .slot = slot;
            }
            eprintln!("New slot: {dst_slot}");

            let ns = self.init_namespace(id.namespace());
            *ns.get_mut(id).expect("Entity is not valid") = EntityLocation {
                slot: dst_slot,
                arch: dst_id,
            };
        }

        Ok(())
    }

    pub fn despawn(&mut self, id: Entity) -> Result<()> {
        let &EntityLocation {
            arch: archetype,
            slot,
        } = self.location(id)?;

        let src = self.archetype_mut(archetype);
        src.remove_slot_changes(slot);
        let swapped = unsafe { src.take(slot, |c, p| (c.drop)(p)) };
        let ns = self.init_namespace(id.namespace());
        if let Some(swapped) = swapped {
            // The last entity in src was moved into the slot occupied by id
            eprintln!("Relocating entity");
            ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        ns.despawn(id)
    }

    pub fn set<T: ComponentValue>(
        &mut self,
        id: impl Into<Entity>,
        component: Component<T>,
        mut value: T,
    ) -> Result<Option<T>> {
        let id = id.into();
        // We know things will change either way
        let change_tick = self.advance_change_tick();

        let EntityLocation { arch: src_id, slot } = self.init_location(id)?;

        let component_info = component.info();

        // if let Some(&EntityLocation { arch: src_id, slot }) = loc {
        let src = self.archetype(src_id);

        if let Some(mut val) = src.get_mut(slot, component) {
            src.changes_mut(component.id())
                .expect("Missing change list")
                .set(Change::modified(Slice::single(slot), change_tick));

            return Ok(Some(mem::replace(&mut *val, value)));
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

                {
                    assert!(components
                        .iter()
                        .sorted_by_key(|v| v.id)
                        .eq(components.iter()));
                }

                let (dst_id, _) = self.fetch_archetype(self.archetype_root, &components);

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
            dst.put_dyn(dst_slot, &component_info, &mut value as *mut T as *mut u8)
                .expect("Insert should not fail");

            // And don't forget to forget to drop it
            std::mem::forget(value);

            assert_eq!(dst.entity(dst_slot), Some(id));

            // Migrate all changes
            src.migrate_slot(dst, slot, dst_slot);

            dst.init_changes(component.id())
                .set(Change::inserted(Slice::single(dst_slot), change_tick));

            if let Some(swapped) = swapped {
                // The last entity in src was moved into the slot occupied by id
                eprintln!("Relocating entity");
                let swapped_ns = self.init_namespace(swapped.namespace());
                swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
            }
            eprintln!("New slot: {dst_slot}");

            let ns = self.init_namespace(id.namespace());
            *ns.get_mut(id).expect("Entity is not valid") = EntityLocation {
                slot: dst_slot,
                arch: dst_id,
            };
        }
        Ok(None)
    }

    pub(crate) fn remove_dyn(&mut self, id: Entity, component: ComponentInfo) -> Result<()> {
        eprintln!("Removing component {component:?} from {id} ");
        unsafe {
            self.remove_inner(id, component, |ptr| {
                eprintln!("Dropping dyn component");
                (component.drop)(ptr)
            })
        }
    }

    unsafe fn remove_inner(
        &mut self,
        id: Entity,
        component: ComponentInfo,
        on_drop: impl FnOnce(*mut u8),
    ) -> Result<()> {
        let ns = self.init_namespace(id.namespace());
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

                let (dst_id, _) = self.fetch_archetype(self.archetype_root, &components);

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
        src.migrate_slot(dst, slot, dst_slot);
        dst.init_changes(component.id())
            .set(Change::removed(Slice::single(dst_slot), change_tick));

        if let Some(swapped) = swapped {
            // The last entity in src was moved into the slot occupied by id
            eprintln!("Relocating entity {swapped}");
            let swapped_ns = self.init_namespace(swapped.namespace());
            swapped_ns.get_mut(swapped).expect("Invalid entity id").slot = slot;
        }

        *self
            .init_namespace(id.namespace())
            .get_mut(id)
            .expect("Entity is not valid") = EntityLocation {
            slot: dst_slot,
            arch: dst_id,
        };

        Ok(())
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
        id: Entity,
        component: Component<T>,
    ) -> Result<AtomicRef<T>> {
        let loc = self.location(id)?;

        self.archetype(loc.arch)
            .get(loc.slot, component)
            .ok_or_else(|| Error::MissingComponent(id, component.name()))
    }

    /// Randomly access an entity's component.
    pub fn get_mut<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Result<AtomicRefMut<T>> {
        let &EntityLocation { arch, slot } = self
            .get_namespace(id.namespace())?
            .get(id)
            .ok_or(Error::NoSuchEntity(id))?;

        let archetype = self.archetype(arch);

        let change_tick = self.advance_change_tick();

        archetype
            .changes_mut(component.id())
            .expect("Change list is empty")
            .set(Change::modified(Slice::single(slot), change_tick));

        archetype
            .get_mut(slot, component)
            .ok_or_else(|| Error::MissingComponent(id, component.name()))
    }

    /// Returns true if the entity has the specified component.
    /// Returns false if
    pub fn has<T: ComponentValue>(&self, id: Entity, component: Component<T>) -> bool {
        if let Ok(loc) = self.location(id) {
            self.archetype(loc.arch).has(component.id())
        } else {
            false
        }
    }

    /// Returns true if the entity is still alive
    pub fn is_alive(&self, id: Entity) -> bool {
        self.get_namespace(id.namespace())
            .map(|v| v.is_alive(id))
            .unwrap_or(false)
    }

    pub(crate) fn archetypes(&self) -> impl Iterator<Item = (ArchetypeId, &Archetype)> {
        self.archetypes.iter()
    }

    /// Returns the location of an entity, or spawns if it is in the static
    /// namespace.
    ///
    /// This is often the case when setting components to components.
    ///
    /// If the entity is not found and is not in the static namespace an error
    /// will be returned.
    pub(crate) fn init_location(&mut self, id: Entity) -> Result<EntityLocation> {
        self.init_namespace(id.namespace())
            .get(id)
            .ok_or(Error::NoSuchEntity(id))
            .copied()
            .or_else(|e| {
                if id.namespace() == STATIC_NAMESPACE {
                    Ok(self.spawn_at(id))
                } else {
                    Err(e)
                }
            })
    }

    pub(crate) fn location(&self, id: Entity) -> Result<&EntityLocation> {
        self.get_namespace(id.namespace())?
            .get(id)
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

    /// Visit all components which have the visitor components and use the
    /// associated visitor for each slot
    pub fn visit<C, V>(&self, visitor: Component<V>, ctx: &mut C)
    where
        V: Visitor<C> + ComponentValue,
    {
        for (_, arch) in self.archetypes.iter() {
            for component in arch.components() {
                if let Ok(mut v) = self.get_mut(component.id(), visitor) {
                    arch.visit(component.id(), &mut *v, ctx);
                }
            }
        }
    }

    pub(crate) fn reconstruct(&self, id: crate::StrippedEntity) -> Option<Entity> {
        let ns = self.get_namespace(id.namespace()).ok()?;

        ns.reconstruct(id).map(|v| v.0)
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{EntityBuilder, Query};

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

        let root = world.archetype_root;

        // () -> (a) -> (ab) -> (abc)
        let (_, archetype) = world.fetch_archetype(root, &[a().info(), b().info(), c().info()]);
        assert!(!archetype.has(d().id()));
        assert!(archetype.has(a().id()));
        assert!(archetype.has(b().id()));

        // dbg!(&world.archetypes);

        // () -> (a) -> (ab) -> (abc)
        //                   -> (abd)
        let (_, archetype) = world.fetch_archetype(root, &[a().info(), b().info(), d().info()]);
        assert!(archetype.has(d().id()));
        assert!(!archetype.has(c().id()));
    }

    #[test]
    fn insert() {
        let mut world = World::new();
        let id = world.spawn();

        world.set(id, a(), 65);
        let shared = Arc::new("Foo".to_string());

        assert_eq!(world.get(id, a()).as_deref(), Ok(&65));
        assert_eq!(
            world.get(id, b()).as_deref(),
            Err(&Error::MissingComponent(id, "b"))
        );
        assert_eq!(world.has(id, c()), false);

        let id2 = world.spawn();
        world.set(id2, a(), 7);

        world.set(id2, c(), "Foo".to_string());

        eprintln!("a: {}, b: {}, c: {}, id: {}", a(), a(), c(), id);

        assert_eq!(world.get(id, a()).as_deref(), Ok(&65));
        assert_eq!(
            world.get(id, b()).as_deref(),
            Err(&Error::MissingComponent(id, "b"))
        );

        assert_eq!(world.has(id, c()), false);
        assert_eq!(world.get(id2, a()).as_deref(), Ok(&7));
        assert_eq!(world.get(id2, c()).as_deref(), Ok(&"Foo".to_string()));
        world.set(id, e(), shared.clone());
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

        world.set(id1, a(), 40);

        world.set(id2, b(), 4.3);

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

        world.set(id, e(), shared.clone());
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
