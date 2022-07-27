use std::mem;

use crate::{
    wildcard, CommandBuffer, Component, ComponentBuffer, ComponentValue, Entity, EntityKind, World,
};

#[derive(Debug)]
pub struct EntityBuilder {
    buffer: ComponentBuffer,
    children: Vec<EntityBuilder>,
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self {
            buffer: ComponentBuffer::new(),
            children: Vec::new(),
        }
    }

    /// Sets the component of the entity.
    pub fn set<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> &mut Self {
        self.buffer.set(component, value);
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

    /// Attach a child with the provided relation and value.
    /// The child is taken and cleared
    pub fn attach_with<T: ComponentValue>(
        &mut self,
        relation: fn(Entity) -> Component<T>,
        value: T,
        other: &mut Self,
    ) -> &mut Self {
        other.set(relation(wildcard()), value);
        self.children.push(mem::take(other));
        self
    }

    /// Attach a child with the provided value-less relation
    pub fn attach(&mut self, relation: fn(Entity) -> Component<()>, other: &mut Self) -> &mut Self {
        self.attach_with(relation, (), other)
    }

    /// Spawns the built entity into the world.
    ///
    /// Clears the builder and allows it to be used again, reusing the builder
    /// will reuse the inner storage, even for different components.
    pub fn spawn(&mut self, world: &mut World) -> Entity {
        let iter = self.buffer.take_all().map(|(info, val)| {
            let id = info.id();
            if id.kind().contains(EntityKind::RELATION) && id.high() == wildcard().low() {
                panic!("Attempt to build entity with an unknown parent, but entity requires a parent relation")
            }

            ( info, val )
        });

        let id = world.spawn_with(iter);

        self.children.drain(..).for_each(|mut child| {
            child.spawn_with_parent(world, id);
        });

        id
    }

    fn spawn_with_parent(&mut self, world: &mut World, parent: Entity) -> Entity {
        let iter = self.buffer.take_all().map(|(mut info, val)| {
            let id = info.id();
            if id.kind().contains(EntityKind::RELATION) && id.high() == wildcard().low() {
                tracing::info!("Inserting parent component");
                let rel = id.low();
                info.id = Entity::join_pair(rel, parent.low())
            }

            (info, val)
        });

        world.spawn_with(iter)
    }

    /// Spawns the entity into the world through a commandbuffer
    pub fn spawn_into(&mut self, cmd: &mut CommandBuffer) {
        cmd.spawn(mem::take(self));
    }
}

impl Default for EntityBuilder {
    fn default() -> Self {
        Self::new()
    }
}
