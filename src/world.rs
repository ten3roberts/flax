use std::{
    mem,
    sync::atomic::{AtomicU32, Ordering},
};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use itertools::Itertools;

use crate::{
    archetype::{Archetype, ArchetypeId, ComponentInfo, Slice},
    entity::{EntityLocation, EntityStore},
    Component, ComponentBuffer, ComponentId, ComponentValue, Entity,
};

pub struct World {
    entities: EntityStore,
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
            entities: EntityStore::new(1),
            archetypes,
            change_tick: AtomicU32::default(),
            archetype_gen: AtomicU32::default(),
            archetype_root: root,
        }
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

    /// Spawn a new empty entity
    pub fn spawn(&mut self) -> Entity {
        // Place at root
        let id = self.entities.spawn(EntityLocation {
            arch: self.archetype_root,
            slot: 0,
        });

        let slot = unsafe { self.archetype_mut(self.archetype_root).allocate(id) };

        // This is safe as `root` does not contain any components
        self.entities.get_mut(id).unwrap().slot = slot;
        id
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
    pub fn spawn_with(&mut self, components: &mut ComponentBuffer) -> Entity {
        let id = self.spawn();

        let (archetype_id, arch) =
            self.fetch_archetype(self.archetype_root, components.components());

        let slot = arch.insert(id, components);
        *self.entities.get_mut(id).unwrap() = EntityLocation {
            arch: archetype_id,
            slot,
        };

        id
    }

    pub fn despawn(&mut self, id: Entity) -> Option<()> {
        let &EntityLocation {
            arch: archetype,
            slot,
        } = self.location(id)?;

        let src = self.archetype_mut(archetype);
        unsafe {
            let swapped = src.take(slot, |c, p| (c.drop)(p));
            if let Some(swapped) = swapped {
                // The last entity in src was moved into the slot occupied by id
                eprintln!("Relocating entity");
                self.entities
                    .get_mut(swapped)
                    .expect("Invalid entity id")
                    .slot = slot;
            }

            self.entities.despawn(id);
        }

        Some(())
    }

    pub fn set<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        mut value: T,
    ) -> Option<T> {
        let &EntityLocation { arch: src_id, slot } = self.entities.get(id).unwrap();
        let src = self.archetype(src_id);

        let component_info = component.info();

        // We know things will change either way
        let change_tick = self.advance_change_tick();

        if let Some(mut val) = src.get_mut(slot, component) {
            src.changes_mut(component.id())?
                .set(Slice::single(slot), change_tick);

            return Some(mem::replace(&mut *val, value));
        }

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
            if let Some(swapped) = swapped {
                // The last entity in src was moved into the slot occupied by id
                eprintln!("Relocating entity");
                self.entities
                    .get_mut(swapped)
                    .expect("Invalid entity id")
                    .slot = slot;
            }
            eprintln!("New slot: {dst_slot}");

            *self.entities.get_mut(id).expect("Entity is not valid") = EntityLocation {
                slot: dst_slot,
                arch: dst_id,
            };
        }

        None
    }

    pub fn remove<T: ComponentValue>(&mut self, id: Entity, component: Component<T>) -> Option<T> {
        let &EntityLocation { arch: src_id, slot } = self.entities.get(id).unwrap();

        let src = self.archetype(src_id);

        if !src.has(component.id()) {
            return None;
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

        unsafe {
            assert_ne!(src_id, dst_id);
            // Borrow disjoint
            let (src, dst) = self.archetypes.get_disjoint(src_id, dst_id).unwrap();

            // Add a quick edge to refer to later
            src.add_edge_to(dst, dst_id, src_id, component.id());

            // Take the value
            // This moves the differing value out of the archetype before it is
            // forgotten in the move

            eprintln!("Moving {id} from {src_id} => {dst_id}");
            let mut val = std::ptr::null();
            // Capture the ONE moved value
            let (dst_slot, swapped) = src.move_to(dst, slot, |_, p| {
                assert_eq!(val as *const u8, std::ptr::null());
                val = p
            });

            assert_ne!(val, std::ptr::null());
            assert_eq!(dst.entity(dst_slot), Some(id));
            if let Some(swapped) = swapped {
                // The last entity in src was moved into the slot occupied by id
                eprintln!("Relocating entity");
                self.entities
                    .get_mut(swapped)
                    .expect("Invalid entity id")
                    .slot = slot;
            }

            *self.entities.get_mut(id).expect("Entity is not valid") = EntityLocation {
                slot: dst_slot,
                arch: dst_id,
            };

            Some(val.cast::<T>().read())
        }
    }

    /// Randomly access an entity's component.
    pub fn get<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        let loc = self.entities.get(id)?;

        self.archetypes.get(loc.arch)?.get(loc.slot, component)
    }

    /// Randomly access an entity's component.
    pub fn get_mut<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Option<AtomicRefMut<T>> {
        let &EntityLocation { arch, slot } = self.entities.get(id)?;

        let archetype = self.archetype(arch);

        let change_tick = self.advance_change_tick();

        archetype
            .changes_mut(component.id())?
            .set(Slice::single(slot), change_tick);

        archetype.get_mut(slot, component)
    }

    /// Returns true if the entity has the specified component.
    /// Returns false if
    pub fn has<T: ComponentValue>(&self, id: Entity, component: Component<T>) -> bool {
        let loc = self.entities.get(id);
        if let Some(loc) = loc {
            self.archetype(loc.arch).has(component.id())
        } else {
            false
        }
    }

    /// Returns true if the entity is still alive
    pub fn is_alive(&self, id: Entity) -> bool {
        self.entities.is_alive(id)
    }

    pub(crate) fn archetypes(&self) -> impl Iterator<Item = (ArchetypeId, &Archetype)> {
        self.archetypes.iter()
    }

    pub(crate) fn location(&self, entity: Entity) -> Option<&EntityLocation> {
        self.entities.get(entity)
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

        assert_eq!(world.get(id, a()).as_deref(), Some(&65));
        assert_eq!(world.get(id, b()).as_deref(), None);
        assert_eq!(world.has(id, c()), false);

        let id2 = world.spawn();
        world.set(id2, a(), 7);

        world.set(id2, c(), "Foo".to_string());

        eprintln!("a: {}, b: {}, c: {}, id: {}", a(), a(), c(), id);

        assert_eq!(world.get(id, a()).as_deref(), Some(&65));
        assert_eq!(world.get(id, b()).as_deref(), None);
        assert_eq!(world.has(id, c()), false);
        assert_eq!(world.get(id2, a()).as_deref(), Some(&7));
        assert_eq!(world.get(id2, c()).as_deref(), Some(&"Foo".to_string()));
        world.set(id, e(), shared.clone());
        assert_eq!(
            world.get(id, e()).as_deref().map(|v| &**v),
            Some(&"Foo".to_string())
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

        // Borrow another component on an entity with a mutably borrowed
        // **other** component.
        assert_eq!(world.get(id2, a()).as_deref(), None);
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

        assert_eq!(world.get(id, b()).as_deref(), Some(&0.3));
        assert_eq!(world.get(id, e()).as_deref(), Some(&shared));

        assert_eq!(world.remove(id, e()).as_ref(), Some(&shared));

        assert_eq!(world.get(id, a()).as_deref(), Some(&9));
        assert_eq!(world.get(id, c()).as_deref(), Some(&"Foo".to_string()));
        assert_eq!(world.get(id, e()).as_deref(), None);

        world.despawn(id).unwrap();

        assert_eq!(world.get(id, a()).as_deref(), None);
        assert_eq!(world.get(id, c()).as_deref(), None);
        assert_eq!(world.get(id, e()).as_deref(), None);

        assert_eq!(world.get(id2, e()).as_deref(), Some(&shared));
        assert_eq!(world.get(id2, c()).as_deref(), Some(&"Bar".to_string()));

        assert_eq!(world.get(id, e()).as_deref(), None);

        assert_eq!(Arc::strong_count(&shared), 2);

        // // Remove id

        let mut query = Query::new((a(), c()));
        let items = query.iter(&world).sorted().collect_vec();

        assert_eq!(items, [(&6, &"Bar".to_string())]);
    }
}
