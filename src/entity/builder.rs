use std::mem;

use crate::{CommandBuffer, Component, ComponentBuffer, ComponentValue, Entity, Namespace, World};

#[derive(Debug)]
pub struct EntityBuilder {
    buffer: ComponentBuffer,
    namespace: Namespace,
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self {
            buffer: ComponentBuffer::new(),
            namespace: 0,
        }
    }

    pub fn with_namespace(&mut self, namespace: Namespace) -> &mut Self {
        self.namespace = namespace;
        self
    }

    /// Sets the component of the entity.
    pub fn set<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> &mut Self {
        self.buffer.insert(component, value);
        self
    }

    /// Shorthand for setting a unit type component
    pub fn tag<T: From<()> + ComponentValue>(&mut self, component: Component<T>) -> &mut Self {
        self.set(component, ().into())
    }

    /// Sets a component with the default value of `T`
    pub fn set_default<T: ComponentValue + Default>(
        &mut self,
        component: Component<T>,
    ) -> &mut Self {
        self.set(component, Default::default())
    }

    /// Return a mutable reference to the stored component.
    pub fn get_mut<T: ComponentValue>(&mut self, component: Component<T>) -> Option<&mut T> {
        self.buffer.get_mut(component)
    }

    /// Return a reference to the stored component.
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Option<&T> {
        self.buffer.get(component)
    }

    /// Spawns the built entity into the world.
    ///
    /// Clears the builder and allows it to be used again, reusing the builder
    /// will reuse the inner storage, even for different components.
    pub fn spawn(&mut self, world: &mut World) -> Entity {
        world.spawn_with(self.namespace, &mut self.buffer)
    }

    /// Spawns the entity into the world through a commandbuffer
    pub fn spawn_into(&mut self, cmd: &mut CommandBuffer) {
        cmd.spawn(self.take());
    }

    /// Takes all components from self and stores them in a new builder.
    /// Effectively stealing everything from the builder by mutable reference.
    pub fn take(&mut self) -> Self {
        Self {
            buffer: mem::take(&mut self.buffer),
            namespace: self.namespace,
        }
    }
}

impl Default for EntityBuilder {
    fn default() -> Self {
        Self::new()
    }
}
