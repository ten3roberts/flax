use core::fmt;

use alloc::{boxed::Box, format, vec::Vec};
use eyre::Context;

use crate::{
    buffer::BufferStorage, BatchSpawn, Component, ComponentInfo, ComponentValue, Entity,
    EntityBuilder, World,
};

type DeferFn = Box<dyn Fn(&mut World) -> eyre::Result<()> + Send + Sync>;

/// A recorded action to be applied to the world.
enum Command {
    /// Spawn a new entity
    Spawn(EntityBuilder),
    AppendTo(EntityBuilder, Entity),
    SpawnAt(EntityBuilder, Entity),
    /// Spawn a batch of entities with the same components
    SpawnBatch(BatchSpawn),
    SpawnBatchAt(BatchSpawn, Vec<Entity>),
    /// Set a component for an entity
    Set {
        id: Entity,
        info: ComponentInfo,
        offset: usize,
    },
    /// Despawn an entity
    Despawn(Entity),
    /// Remove a component from an entity
    Remove {
        id: Entity,
        info: ComponentInfo,
    },

    /// Execute an arbitrary function with a mutable reference to the world.
    Defer(DeferFn),
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(v) => f.debug_tuple("Spawn").field(v).finish(),
            Self::SpawnAt(id, v) => f.debug_tuple("SpawnAt").field(&v).field(&id).finish(),
            Self::AppendTo(id, v) => f.debug_tuple("AppendTo").field(&v).field(&id).finish(),
            Self::SpawnBatch(batch) => f.debug_tuple("SpawnBatch").field(batch).finish(),
            Self::SpawnBatchAt(batch, ids) => f
                .debug_tuple("SpawnBatchAt")
                .field(&batch)
                .field(&ids.len())
                .finish(),
            Self::Set { id, info, offset } => f
                .debug_struct("Set")
                .field("id", id)
                .field("info", info)
                .field("offset", offset)
                .finish(),
            Self::Despawn(arg0) => f.debug_tuple("Despawn").field(arg0).finish(),
            Self::Remove {
                id,
                info: component,
            } => f
                .debug_struct("Remove")
                .field("id", id)
                .field("component", component)
                .finish(),
            Self::Defer(_) => f.debug_tuple("Defer").field(&"...").finish(),
        }
    }
}

/// Records commands into the world.
/// Allows insertion and removal of components when the world is not available
/// mutably, such as in systems or during iteration.
#[derive(Default)]
pub struct CommandBuffer {
    inserts: BufferStorage,
    commands: Vec<Command>,
}

impl fmt::Debug for CommandBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandBuffer")
            .field("commands", &self.commands)
            .finish()
    }
}

/// Since all components are Send + Sync, the commandbuffer is as well
unsafe impl Send for CommandBuffer {}
unsafe impl Sync for CommandBuffer {}

impl CommandBuffer {
    /// Creates a new commandbuffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Deferred set a component for `id`.
    /// Unlike, [`World::set`] it does not return the old value as that is
    /// not known at call time.
    pub fn set<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> &mut Self {
        let offset = self.inserts.insert(value);
        self.commands.push(Command::Set {
            id,
            info: component.info(),
            offset,
        });

        self
    }

    /// Deferred removal of a component for `id`.
    /// Unlike, [`World::remove`] it does not return the old value as that is
    /// not known at call time.
    pub fn remove<T: ComponentValue>(&mut self, id: Entity, component: Component<T>) -> &mut Self {
        self.commands.push(Command::Remove {
            id,
            info: component.info(),
        });

        self
    }

    /// Spawn a new entity with the given components of the builder
    pub fn spawn(&mut self, entity: impl Into<EntityBuilder>) -> &mut Self {
        self.commands.push(Command::Spawn(entity.into()));

        self
    }

    /// Spawn a new entity with the given components of the builder
    pub fn spawn_at(&mut self, id: Entity, entity: impl Into<EntityBuilder>) -> &mut Self {
        self.commands.push(Command::SpawnAt(entity.into(), id));

        self
    }

    /// Append components to an existing entity
    pub fn append_to(&mut self, id: Entity, entity: impl Into<EntityBuilder>) -> &mut Self {
        self.commands.push(Command::AppendTo(entity.into(), id));

        self
    }

    /// Spawn a new batch with the given components of the builder
    pub fn spawn_batch(&mut self, batch: impl Into<BatchSpawn>) -> &mut Self {
        self.commands.push(Command::SpawnBatch(batch.into()));

        self
    }

    /// Spawn a new batch with the given components of the builder
    pub fn spawn_batch_at(&mut self, ids: Vec<Entity>, batch: impl Into<BatchSpawn>) -> &mut Self {
        self.commands.push(Command::SpawnBatchAt(batch.into(), ids));

        self
    }

    /// Despawn an entity by id
    pub fn despawn(&mut self, id: Entity) -> &mut Self {
        self.commands.push(Command::Despawn(id));
        self
    }

    /// Defer a function to execute upon the world.
    ///
    /// Errors will be propagated.
    pub fn defer(
        &mut self,
        func: impl Fn(&mut World) -> eyre::Result<()> + Send + Sync + 'static,
    ) -> &mut Self {
        self.commands.push(Command::Defer(Box::new(func)));
        self
    }

    /// Applies all contents of the command buffer to the world.
    /// The commandbuffer is cleared and can be reused.
    pub fn apply(&mut self, world: &mut World) -> eyre::Result<()> {
        for cmd in self.commands.drain(..) {
            match cmd {
                Command::Spawn(mut entity) => {
                    entity.spawn(world);
                }
                Command::SpawnAt(mut entity, id) => {
                    entity
                        .spawn_at(world, id)
                        .map_err(|v| v.into_eyre())
                        .wrap_err("Failed to spawn entity")?;
                }
                Command::AppendTo(mut entity, id) => {
                    entity
                        .append_to(world, id)
                        .map_err(|v| v.into_eyre())
                        .wrap_err("Failed to append to entity")?;
                }
                Command::SpawnBatch(mut batch) => {
                    batch.spawn(world);
                }
                Command::SpawnBatchAt(mut batch, ids) => {
                    batch
                        .spawn_at(world, &ids)
                        .map_err(|v| v.into_eyre())
                        .wrap_err("Failed to spawn entity")?;
                }
                Command::Set { id, info, offset } => unsafe {
                    let value = self.inserts.take_dyn(offset);
                    world
                        .set_dyn(id, info, value, |v| info.drop(v.cast()))
                        .map_err(|v| v.into_eyre())
                        .wrap_err_with(|| format!("Failed to set component {}", info.name()))?;
                },
                Command::Despawn(id) => world
                    .despawn(id)
                    .map_err(|v| v.into_eyre())
                    .wrap_err("Failed to despawn entity")?,
                Command::Remove { id, info } => world
                    .remove_dyn(id, info)
                    .map_err(|v| v.into_eyre())
                    .wrap_err_with(|| format!("Failed to remove component {}", info.name()))?,
                Command::Defer(func) => {
                    func(world).wrap_err("Failed to execute deferred function")?
                }
            }
        }

        Ok(())
    }

    /// Clears all values in the component buffer but keeps allocations around.
    /// Is automatically called for [`Self::apply`].
    pub fn clear(&mut self) {
        self.inserts.clear();
        self.commands.clear()
    }
}
