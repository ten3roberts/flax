use atomic_refcell::{AtomicRef, AtomicRefMut};
use itertools::Itertools;

use crate::{
    archetype::{Archetype, ArchetypeId, ComponentInfo},
    entity::{EntityLocation, EntityStore},
    Component, ComponentBuffer, ComponentId, ComponentValue, Entity,
};

pub struct World {
    entities: EntityStore,
    archetypes: Vec<Archetype>,
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: EntityStore::new(),
            archetypes: vec![Archetype::empty()],
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
            let next = self.archetypes[cursor as usize].edge_to(*head)?;
            cursor = next;
            components = tail;
        }

        Some(&self.archetypes[cursor as usize])
    }

    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    pub fn fetch_archetype<'a>(
        &mut self,
        root: ArchetypeId,
        components: impl IntoIterator<Item = &'a ComponentInfo>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut components = components.into_iter();
        let mut cursor = root;

        let mut i = 0;

        while let Some(head) = components.next() {
            let id = self.archetypes.len() as u32;
            let cur = &mut self.archetypes[cursor as usize];
            cursor = match cur.edge_to(head.id) {
                Some(id) => id,
                None => {
                    let mut new = Archetype::new(cur.components().copied().chain([*head]));

                    cur.add_edge_to(&mut new, id, cursor, head.id);

                    self.archetypes.push(new);
                    id
                }
            };

            i += 1;
        }

        (cursor, &mut self.archetypes[cursor as usize])
    }

    /// Spawn a new empty entity
    pub fn spawn(&mut self) -> Entity {
        // Place at root
        let id = self.entities.spawn(EntityLocation::default());
        // This is safe as `root` does not contain any components
        let slot = unsafe { self.archetype_mut(0).allocate(id) };
        self.entities.get_mut(id).unwrap().slot = slot;
        id
    }

    /// Access an archetype by id
    pub fn archetype(&self, id: ArchetypeId) -> &Archetype {
        &self.archetypes[id as usize]
    }

    /// Access an archetype by id
    pub fn archetype_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        &mut self.archetypes[id as usize]
    }

    /// Spawn an entity with the given components.
    ///
    /// For increased ergonomics, prefer [crate::EntityBuilder]
    pub fn spawn_with(&mut self, components: &mut ComponentBuffer) -> Entity {
        let id = self.spawn();

        let (archetype_id, arch) = self.fetch_archetype(0, components.components());

        let slot = arch.insert(id, components);
        *self.entities.get_mut(id).unwrap() = EntityLocation {
            archetype: archetype_id,
            slot,
        };

        id
    }

    pub fn insert<T: ComponentValue>(&mut self, id: Entity, component: Component<T>, mut value: T) {
        let &EntityLocation {
            archetype: src_id,
            slot,
        } = self.entities.get(id).unwrap();
        let src = self.archetype(src_id);

        let component_info = component.info();
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

                let (dst_id, _) = self.fetch_archetype(0, &components);

                dst_id
            }
        };

        unsafe {
            assert_ne!(src_id, dst_id);
            // Borrow disjoint
            let src =
                &mut *((&self.archetypes[src_id as usize]) as *const Archetype as *mut Archetype);
            let dst =
                &mut *((&self.archetypes[dst_id as usize]) as *const Archetype as *mut Archetype);

            let (dst_slot, swapped) = src.move_to(dst, slot);

            // Insert the missing component
            dst.put_dyn(dst_slot, &component_info, &mut value as *mut T as *mut u8)
                .expect("Insert should not fail");

            // And don't forget to forget to drop it
            std::mem::forget(value);

            // Add a quick edge to refer to later
            src.add_edge_to(dst, dst_id, src_id, component.id());

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
                archetype: dst_id,
            };
        }
    }

    /// Randomly access an entity's component.
    pub fn get<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        let loc = self.entities.get(id)?;

        self.archetypes[loc.archetype as usize].get(loc.slot, component)
    }

    /// Randomly access an entity's component.
    pub fn get_mut<T: ComponentValue>(
        &self,
        id: Entity,
        component: Component<T>,
    ) -> Option<AtomicRefMut<T>> {
        let loc = self.entities.get(id)?;

        self.archetypes[loc.archetype as usize].get_mut(loc.slot, component)
    }

    /// Returns true if the entity has the specified component.
    /// Returns false if
    pub fn has<T: ComponentValue>(&self, id: Entity, component: Component<T>) -> bool {
        let loc = self.entities.get(id);
        if let Some(loc) = loc {
            self.archetype(loc.archetype).has(component.id())
        } else {
            false
        }
    }

    /// Despawns an entity
    pub fn despawn(&mut self, id: Entity) {
        self.entities.despawn(id)
    }

    /// Returns true if the entity is still alive
    pub fn is_alive(&self, id: Entity) -> bool {
        self.entities.is_alive(id)
    }

    pub(crate) fn archetypes(&self) -> impl Iterator<Item = (ArchetypeId, &Archetype)> {
        self.archetypes
            .iter()
            .enumerate()
            .map(|(i, v)| (i as ArchetypeId, v))
    }

    pub(crate) fn location(&self, entity: Entity) -> Option<&EntityLocation> {
        self.entities.get(entity)
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
        let (_, archetype) = world.fetch_archetype(0, &[a().info(), b().info(), c().info()]);
        assert!(!archetype.has(d().id()));
        assert!(archetype.has(a().id()));
        assert!(archetype.has(b().id()));

        // dbg!(&world.archetypes);

        // () -> (a) -> (ab) -> (abc)
        //                   -> (abd)
        let (_, archetype) = world.fetch_archetype(0, &[a().info(), b().info(), d().info()]);
        assert!(archetype.has(d().id()));
        assert!(!archetype.has(c().id()));
    }

    #[test]
    fn insert() {
        let mut world = World::new();
        let id = world.spawn();

        world.insert(id, a(), 65);
        let shared = Arc::new("Foo".to_string());

        assert_eq!(world.get(id, a()).as_deref(), Some(&65));
        assert_eq!(world.get(id, b()).as_deref(), None);
        assert_eq!(world.has(id, c()), false);

        let id2 = world.spawn();
        world.insert(id2, a(), 7);

        world.insert(id2, c(), "Foo".to_string());

        eprintln!("a: {}, b: {}, c: {}, id: {}", a(), a(), c(), id);

        assert_eq!(world.get(id, a()).as_deref(), Some(&65));
        assert_eq!(world.get(id, b()).as_deref(), None);
        assert_eq!(world.has(id, c()), false);
        assert_eq!(world.get(id2, a()).as_deref(), Some(&7));
        assert_eq!(world.get(id2, c()).as_deref(), Some(&"Foo".to_string()));
        world.insert(id, e(), shared.clone());
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

        world.insert(id1, a(), 40);

        world.insert(id2, b(), 4.3);

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
}
