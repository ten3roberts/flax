use std::collections::{btree_map::Entry, BTreeMap};

use eyre::Context;
use itertools::Itertools;

use crate::{
    buffer::BufferStorage, BatchSpawn, Component, ComponentInfo, ComponentValue, Entity,
    EntityBuilder, World,
};

type DeferFn = Box<dyn Fn(&mut World) -> eyre::Result<()> + Send + Sync>;

enum Command {
    Spawn(EntityBuilder),
    SpawnBatch(BatchSpawn),
    Set {
        id: Entity,
        info: ComponentInfo,
        offset: usize,
    },
    Despawn(Entity),
    Remove {
        id: Entity,
        info: ComponentInfo,
    },

    Defer(DeferFn),
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(arg0) => f.debug_tuple("Spawn").field(arg0).finish(),
            Self::SpawnBatch(arg0) => f.debug_tuple("SpawnBatch").field(arg0).finish(),
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

impl std::fmt::Debug for CommandBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

    /// Spawn a new batch with the given components of the builder
    pub fn spawn_batch(&mut self, batch: BatchSpawn) -> &mut Self {
        self.commands.push(Command::SpawnBatch(batch));

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
                Command::SpawnBatch(mut batch) => {
                    batch.spawn(world);
                }
                Command::Set { id, info, offset } => unsafe {
                    let value = self.inserts.take_dyn(offset);
                    world
                        .set_dyn(id, info, value, |v| (info.drop)(v.cast()))
                        .wrap_err_with(|| format!("Failed to set component {}", info.name()))?;
                },
                Command::Despawn(id) => world.despawn(id).wrap_err("Failed to despawn entity")?,
                Command::Remove { id, info } => world
                    .remove_dyn(id, info)
                    .wrap_err_with(|| format!("Failed to remove component {}", info.name))?,
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
