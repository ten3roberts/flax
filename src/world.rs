use crate::{entity::EntityStore, Entity};

pub struct World {
    entities: EntityStore,
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: EntityStore::new(),
        }
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
