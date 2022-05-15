use crate::{
    archetype::{Archetype, ComponentInfo},
    entity::EntityStore,
    ComponentId, Entity,
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
    pub fn find_archetype(&self, mut components: &[ComponentId]) -> Option<&Archetype> {
        let mut cursor = 0;

        while let [head, tail @ ..] = components {
            let next = self.archetypes[cursor as usize].edge_to(*head)?;
            cursor = next;
            components = tail;
        }

        Some(&self.archetypes[cursor as usize])
    }

    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    pub fn fetch_archetype(&mut self, mut components: &[ComponentInfo]) -> &mut Archetype {
        let mut cursor = 0;

        let all = components;
        let mut i = 0;

        while let [head, tail @ ..] = components {
            let id = self.archetypes.len() as u32;
            let cur = &mut self.archetypes[cursor as usize];
            cursor = match cur.edge_to(head.id) {
                Some(id) => id,
                None => {
                    // Create archetype
                    eprintln!(
                        "Creating new archetype {:?} => {}\n {:#?}",
                        cur.components().last().map(|v| v.name),
                        head.name,
                        &all[..=i]
                    );
                    let mut new = Archetype::new(all[..=i].to_vec());

                    cur.add_edge_to(&mut new, id, cursor, head.id);

                    self.archetypes.push(new);
                    id
                }
            };
            components = tail;

            i += 1;
        }

        &mut self.archetypes[cursor as usize]
    }

    /// Spawn a new empty entity
    pub fn spawn(&mut self) -> Entity {
        self.entities.spawn()
    }

    /// Despawns an entity
    pub fn despawn(&mut self, id: Entity) {
        self.entities.despawn(id)
    }

    /// Returns true if the entity is still alive
    pub fn is_alive(&self, id: Entity) -> bool {
        self.entities.is_alive(id)
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    component! {
        a: i32,
        b: f32,
        c: String,
        d: Vec<u32>,
    }

    #[test]
    fn world_archetype_graph() {
        let mut world = World::new();

        // () -> (a) -> (ab) -> (abc)
        let archetype = world.fetch_archetype(&[a().info(), b().info(), c().info()]);
        assert!(!archetype.has(d().id()));
        assert!(archetype.has(a().id()));
        assert!(archetype.has(b().id()));

        // dbg!(&world.archetypes);

        // () -> (a) -> (ab) -> (abc)
        //                   -> (abd)
        let archetype = world.fetch_archetype(&[a().info(), b().info(), d().info()]);
        assert!(archetype.has(d().id()));
        assert!(!archetype.has(c().id()));
    }
}
