use crate::{Component, ComponentBuffer, ComponentValue, Entity, World};

pub struct EntityBuilder {
    buffer: ComponentBuffer,
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self {
            buffer: ComponentBuffer::new(),
        }
    }

    /// Sets the component of the entity.
    pub fn set<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> &mut Self {
        self.buffer.insert(component, value);
        self
    }

    /// Spawns the build entities into the world.
    ///
    /// Clears the builder and allows it to be used again, reusing the builder
    /// will reuse the inner storage, even for different components.
    pub fn spawn(&mut self, world: &mut World) -> Entity {
        world.spawn_with(&mut self.buffer)
    }
}
