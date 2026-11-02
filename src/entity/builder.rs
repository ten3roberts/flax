use crate::{
    buffer::ComponentBuffer,
    component::{ComponentDesc, ComponentKey, ComponentValue},
    error::Result,
    relation::RelationExt,
    CommandBuffer, Component, Entity, World,
};
use alloc::{boxed::Box, vec::Vec};

type ModifyFunc = Box<dyn FnOnce(Entity, &mut EntityBuilder) + Send + Sync>;

/// Attached child builder with relation
pub struct ChildEntityBuilder {
    builder: EntityBuilder,
    modify: ModifyFunc,
}

impl core::ops::DerefMut for ChildEntityBuilder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.builder
    }
}

impl core::ops::Deref for ChildEntityBuilder {
    type Target = EntityBuilder;

    fn deref(&self) -> &Self::Target {
        &self.builder
    }
}

impl core::fmt::Debug for ChildEntityBuilder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.builder.fmt(f)
    }
}

impl ChildEntityBuilder {
    fn spawn(mut self, world: &mut World, parent: Entity) -> Entity {
        (self.modify)(parent, &mut self.builder);
        self.builder.spawn(world)
    }
}

#[derive(Debug)]
/// Incrementally build a single entity which allows for more efficient
/// insertion into the world.
///
/// ```rust
/// # use flax::{*, components::*};
/// # use glam::*;
/// # component! {
/// #     health: f32,
/// #     position: Vec3,
/// #     is_player: (),
/// # }
/// # let mut world = World::new();
/// let id = Entity::builder()
///     .set(name(), "Player".into())
///     .set(position(), vec3(0.0, 4.0, 2.0))
///     .set(health(), 100.0)
///     .tag(is_player())
///     .spawn(&mut world);
/// ```
pub struct EntityBuilder {
    buffer: ComponentBuffer,
    children: Vec<ChildEntityBuilder>,
}

impl EntityBuilder {
    /// Creates a new entity builder.
    ///
    /// Prefer [`Entity::builder`](crate::Entity::builder)
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

    pub(crate) unsafe fn set_dyn(&mut self, desc: ComponentDesc, value: *mut u8) -> &mut Self {
        self.buffer.set_dyn(desc, value);
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

    /// Convenience function for only setting the component if Some.
    pub fn set_opt<T: ComponentValue>(
        &mut self,
        component: Component<T>,
        value: Option<T>,
    ) -> &mut Self {
        if let Some(value) = value {
            self.buffer.set(component, value);
        }
        self
    }

    /// Return a mutable reference to the stored component.
    pub fn get_mut<T: ComponentValue>(&mut self, component: Component<T>) -> Option<&mut T> {
        self.buffer.get_mut(component)
    }

    /// Return a reference to the stored component.
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Option<&T> {
        self.buffer.get(component)
    }

    /// Returns true if the entity builder contains the given component
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.buffer.has(component)
    }

    /// Remove a component from the component buffer
    pub fn remove<T: ComponentValue>(&mut self, component: Component<T>) -> Option<T> {
        self.buffer.remove(component)
    }

    /// Retains only the components for which `f` returns true, and drops the rest
    ///
    /// This does not affect the attached children, which are reached through
    /// [`Self::children_mut`]. The closure is called once for every component, and may be
    /// stateful.
    ///
    /// ```rust
    /// # use flax::*;
    /// component! {
    ///     name: String,
    ///     health: f32,
    /// }
    ///
    /// let mut builder = Entity::builder();
    /// builder.set(name(), "player".into()).set(health(), 100.0);
    ///
    /// builder.retain(|key| key == name().key());
    /// assert!(builder.has(name()));
    /// assert!(!builder.has(health()));
    /// ```
    pub fn retain(&mut self, f: impl FnMut(ComponentKey) -> bool) -> &mut Self {
        self.buffer.retain(f);
        self
    }

    /// Attach a child with the provided relation and value.
    /// The child is taken and cleared
    pub fn attach_with<T: ComponentValue>(
        &mut self,
        relation: impl RelationExt<T> + ComponentValue,
        value: T,
        other: impl Into<Self>,
    ) -> &mut Self {
        self.children.push(ChildEntityBuilder {
            builder: other.into(),
            modify: Box::new(move |parent, builder| {
                builder.set(relation.of(parent), value);
            }),
        });
        self
    }

    /// Attach a child with the default value
    pub fn attach<T: ComponentValue + Default>(
        &mut self,
        relation: impl RelationExt<T> + ComponentValue,
        other: impl Into<Self>,
    ) -> &mut Self {
        self.attach_with(relation, Default::default(), other)
    }

    /// Moves all components from `other` into `self`, leaving `other` empty.
    ///
    /// Duplicate components will be overwritten
    pub fn append(&mut self, other: &mut Self) -> &mut Self {
        self.buffer.append(&mut other.buffer);
        self
    }

    /// Spawns the built entity into the world.
    ///
    /// Clears the builder and allows it to be used again, reusing the builder
    /// will reuse the inner storage, even for different components.
    pub fn spawn(&mut self, world: &mut World) -> Entity {
        profile_function!();
        let id = world.spawn_with(&mut self.buffer);

        self.children.drain(..).for_each(|child| {
            child.spawn(world, id);
        });

        id
    }

    /// See: [`Self::spawn`]
    ///
    /// Spawn at a specific entity.
    ///
    /// Fails if an entity with the same index already exists.
    pub fn spawn_at(&mut self, world: &mut World, id: Entity) -> Result<Entity> {
        let (id, _) = world.spawn_at_with(id, &mut self.buffer)?;

        self.children.drain(..).for_each(|child| {
            child.spawn(world, id);
        });

        Ok(id)
    }

    /// Appends the components in the builder to an existing entity.
    ///
    /// New components will overwrite existing components.
    pub fn append_to(&mut self, world: &mut World, id: Entity) -> Result<Entity> {
        profile_function!();
        world.set_with(id, &mut self.buffer)?;

        self.children.drain(..).for_each(|child| {
            child.spawn(world, id);
        });

        Ok(id)
    }

    /// Spawns the entity into the world through a commandbuffer
    pub fn spawn_into(&mut self, cmd: &mut CommandBuffer) {
        cmd.spawn(core::mem::take(self));
    }

    /// Returns the number of component in the builder
    pub fn component_count(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true if the builder does not contain any components
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the attached child builders
    pub fn children(&self) -> &[ChildEntityBuilder] {
        &self.children
    }

    /// Returns the attached child builders
    pub fn children_mut(&mut self) -> &mut Vec<ChildEntityBuilder> {
        &mut self.children
    }
}

impl Default for EntityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<&mut EntityBuilder> for EntityBuilder {
    fn from(builder: &mut EntityBuilder) -> Self {
        core::mem::take(builder)
    }
}

#[cfg(test)]
mod test {
    use crate::{component, components::name, error::MissingComponent, Entity, Error, World};

    #[test]
    fn builder() {
        use glam::*;

        component! {
            health: f32,
            position: Vec3,
            is_player: (),
            is_enemy: (),
        }

        let mut world = World::new();
        let mut builder = Entity::builder();

        builder
            .set(name(), "Player".into())
            .set(position(), vec3(0.0, 4.0, 2.0))
            .set_opt(is_enemy(), None)
            .set_opt(health(), Some(100.0))
            .tag(is_player());

        assert_eq!(builder.get(name()), Some(&"Player".into()));
        assert_eq!(builder.get(health()), Some(&100.0));
        builder.remove(health());
        assert_eq!(builder.get(health()), None);

        builder.set(health(), 50.0);
        let id = builder.spawn(&mut world);

        assert_eq!(world.get(id, name()).as_deref(), Ok(&"Player".into()));
        assert_eq!(world.get(id, health()).as_deref(), Ok(&50.0));
        assert_eq!(
            world.get(id, is_enemy()).as_deref(),
            Err(&Error::MissingComponent(MissingComponent {
                id,
                desc: is_enemy().desc()
            }))
        );
    }
}

#[cfg(test)]
mod retain_test {
    use alloc::sync::Arc;

    use crate::{component, Entity, World};

    component! {
        kept: String,
        owned: Arc<()>,
        health: f32,
    }

    #[test]
    fn retain_drops_the_rest_and_keeps_the_kept_spawnable() {
        let shared = Arc::new(());
        let mut builder = Entity::builder();
        builder
            .set(kept(), "player".into())
            .set(owned(), shared.clone())
            .set(health(), 100.0);
        assert_eq!(Arc::strong_count(&shared), 2);

        let mut asked = 0;
        builder.retain(|key| {
            asked += 1;
            key == kept().key()
        });

        assert_eq!(asked, 3);
        assert_eq!(Arc::strong_count(&shared), 1, "the removed value is dropped once");
        assert!(builder.has(kept()) && !builder.has(owned()) && !builder.has(health()));

        let mut world = World::new();
        let id = builder.spawn(&mut world);
        assert_eq!(world.get(id, kept()).as_deref(), Ok(&"player".to_string()));
        assert!(world.get(id, health()).is_err());
    }
}
