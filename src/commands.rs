use core::fmt;

use alloc::{boxed::Box, format, vec::Vec};
use anyhow::Context;

use crate::{
    buffer::MultiComponentBuffer,
    component::{dummy, ComponentDesc, ComponentValue},
    writer::{MissingDyn, SingleComponentWriter, WriteDedupDyn},
    BatchSpawn, Component, Entity, EntityBuilder, RelationExt, World,
};

type DeferFn = Box<dyn Fn(&mut World) -> anyhow::Result<()> + Send + Sync>;

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
        desc: ComponentDesc,
        offset: usize,
    },
    SetDedup {
        id: Entity,
        desc: ComponentDesc,
        offset: usize,
        cmp: unsafe fn(*const u8, *const u8) -> bool,
    },
    SetMissing {
        id: Entity,
        desc: ComponentDesc,
        offset: usize,
    },
    /// Despawn an entity
    Despawn(Entity),
    DespawnRecursive(ComponentDesc, Entity),
    /// Remove a component from an entity
    Remove {
        id: Entity,
        desc: ComponentDesc,
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
            Self::Set { id, desc, offset } => f
                .debug_struct("Set")
                .field("id", id)
                .field("desc", desc)
                .field("offset", offset)
                .finish(),
            Self::SetDedup {
                id,
                desc,
                offset,
                cmp: _,
            } => f
                .debug_struct("SetDedup")
                .field("id", id)
                .field("desc", desc)
                .field("offset", offset)
                .finish(),
            Self::SetMissing { id, desc, offset } => f
                .debug_struct("SetMissing")
                .field("id", id)
                .field("desc", desc)
                .field("offset", offset)
                .finish(),
            Self::Despawn(arg0) => f.debug_tuple("Despawn").field(arg0).finish(),
            Self::DespawnRecursive(relation, arg0) => f
                .debug_tuple("DespawnRecursive")
                .field(relation)
                .field(arg0)
                .finish(),
            // Self::DespawnRecursive(arg0) => f.debug_tuple("Despawn").field(arg0).finish(),
            Self::Remove {
                id,
                desc: component,
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
    inserts: MultiComponentBuffer,
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

    /// Set a component for `id`.
    pub fn set<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> &mut Self {
        let offset = self.inserts.push(value);
        self.commands.push(Command::Set {
            id,
            desc: component.desc(),
            offset,
        });

        self
    }

    /// Convenience function for only setting the component if Some.
    pub fn set_opt<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: Option<T>,
    ) -> &mut Self {
        if let Some(value) = value {
            self.set(id, component, value);
        }
        self
    }

    /// Set a component for `id`.
    ///
    /// Does not trigger a modification event if the value is the same
    pub fn set_dedup<T: ComponentValue + PartialEq>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> &mut Self {
        let offset = self.inserts.push(value);
        unsafe fn cmp<T: PartialEq>(a: *const u8, b: *const u8) -> bool {
            let a = &*(a as *const T);
            let b = &*(b as *const T);

            a == b
        }
        self.commands.push(Command::SetDedup {
            id,
            desc: component.desc(),
            offset,
            cmp: cmp::<T>,
        });

        self
    }

    /// Set a component for `id` if it does not exist when the commandbuffer is applied.
    ///
    /// This avoid accidentally overwriting a component that was added by another system.
    pub fn set_missing<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> &mut Self {
        let offset = self.inserts.push(value);
        self.commands.push(Command::SetMissing {
            id,
            desc: component.desc(),
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
            desc: component.desc(),
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
    pub fn spawn_batch(&mut self, chunk: impl Into<BatchSpawn>) -> &mut Self {
        self.commands.push(Command::SpawnBatch(chunk.into()));

        self
    }

    /// Spawn a new batch with the given components of the builder
    pub fn spawn_batch_at(&mut self, ids: Vec<Entity>, chunk: impl Into<BatchSpawn>) -> &mut Self {
        self.commands.push(Command::SpawnBatchAt(chunk.into(), ids));

        self
    }

    /// Despawn an entity by id
    pub fn despawn(&mut self, id: Entity) -> &mut Self {
        self.commands.push(Command::Despawn(id));
        self
    }

    /// Despawn an entity by id recursively
    pub fn despawn_recursive<T: ComponentValue>(
        &mut self,
        relation: impl RelationExt<T>,
        id: Entity,
    ) -> &mut Self {
        self.commands
            .push(Command::DespawnRecursive(relation.of(dummy()).desc(), id));
        self
    }

    /// Defer a function to execute upon the world.
    ///
    /// Errors will be propagated.
    pub fn defer(
        &mut self,
        func: impl Fn(&mut World) -> anyhow::Result<()> + Send + Sync + 'static,
    ) -> &mut Self {
        self.commands.push(Command::Defer(Box::new(func)));
        self
    }

    /// Applies all contents of the command buffer to the world.
    /// The commandbuffer is cleared and can be reused.
    pub fn apply(&mut self, world: &mut World) -> anyhow::Result<()> {
        for cmd in self.commands.drain(..) {
            match cmd {
                Command::Spawn(mut entity) => {
                    entity.spawn(world);
                }
                Command::SpawnAt(mut entity, id) => {
                    entity
                        .spawn_at(world, id)
                        .map_err(|v| v.into_anyhow())
                        .context("Failed to spawn entity")?;
                }
                Command::AppendTo(mut entity, id) => {
                    entity
                        .append_to(world, id)
                        .map_err(|v| v.into_anyhow())
                        .context("Failed to append to entity")?;
                }
                Command::SpawnBatch(mut batch) => {
                    batch.spawn(world);
                }
                Command::SpawnBatchAt(mut batch, ids) => {
                    batch
                        .spawn_at(world, &ids)
                        .map_err(|v| v.into_anyhow())
                        .context("Failed to spawn entity")?;
                }
                Command::Set { id, desc, offset } => unsafe {
                    let value = self.inserts.take_dyn(offset);
                    world
                        .set_dyn(id, desc, value)
                        .map_err(|v| v.into_anyhow())
                        .with_context(|| format!("Failed to set component {}", desc.name()))?;
                },
                Command::SetDedup {
                    id,
                    desc,
                    offset,
                    cmp,
                } => unsafe {
                    let value = self.inserts.take_dyn(offset);
                    world
                        .set_with_writer(
                            id,
                            SingleComponentWriter::new(desc, WriteDedupDyn { value, cmp }),
                        )
                        .map_err(|v| v.into_anyhow())
                        .with_context(|| format!("Failed to set component {}", desc.name()))?;
                },
                Command::SetMissing { id, desc, offset } => unsafe {
                    let value = self.inserts.take_dyn(offset);
                    world
                        .set_with_writer(id, SingleComponentWriter::new(desc, MissingDyn { value }))
                        .map_err(|v| v.into_anyhow())
                        .with_context(|| format!("Failed to set component {}", desc.name()))?;
                },
                Command::Despawn(id) => world
                    .despawn(id)
                    .map_err(|v| v.into_anyhow())
                    .context("Failed to despawn entity")?,
                Command::DespawnRecursive(relation, id) => world
                    .despawn_recursive_untyped(id, relation.key().id())
                    .map_err(|v| v.into_anyhow())
                    .context("Failed to despawn entity")?,
                Command::Remove { id, desc } => world
                    .remove_dyn(id, desc)
                    .map_err(|v| v.into_anyhow())
                    .with_context(|| format!("Failed to remove component {}", desc.name()))?,
                Command::Defer(func) => {
                    func(world).context("Failed to execute deferred function")?
                }
            }
        }

        self.inserts.clear();

        Ok(())
    }

    /// Clears all values in the component buffer but keeps allocations around.
    /// Is automatically called for [`Self::apply`].
    pub fn clear(&mut self) {
        self.inserts.clear();
        self.commands.clear()
    }
}

#[cfg(test)]
mod tests {
    use crate::{component, FetchExt, Query};

    use super::*;

    #[test]
    fn set_missing() {
        use alloc::string::String;
        use alloc::string::ToString;

        component! {
            a: String,
        }

        let mut world = World::new();
        let mut cmd = CommandBuffer::new();

        let mut query = Query::new((a().modified().satisfied(), a().cloned()));

        let id = EntityBuilder::new().spawn(&mut world);

        assert!(query.collect_vec(&world).is_empty());

        cmd.set_missing(id, a(), "Foo".into())
            .set_missing(id, a(), "Bar".into());

        cmd.apply(&mut world).unwrap();

        assert_eq!(query.collect_vec(&world), [(true, "Foo".to_string())]);
        assert_eq!(query.collect_vec(&world), [(false, "Foo".to_string())]);

        cmd.set_missing(id, a(), "Baz".into());
        cmd.apply(&mut world).unwrap();

        assert_eq!(query.collect_vec(&world), [(false, "Foo".to_string())]);
    }

    #[test]
    fn set_dedup() {
        use alloc::string::String;
        use alloc::string::ToString;

        component! {
            a: String,
        }

        let mut world = World::new();
        let mut cmd = CommandBuffer::new();

        let mut query = Query::new((a().modified().satisfied(), a().cloned()));

        let id = EntityBuilder::new().spawn(&mut world);

        assert!(query.collect_vec(&world).is_empty());

        cmd.set_dedup(id, a(), "Foo".into())
            .set_dedup(id, a(), "Bar".into());

        cmd.apply(&mut world).unwrap();

        assert_eq!(query.collect_vec(&world), [(true, "Bar".to_string())]);
        assert_eq!(query.collect_vec(&world), [(false, "Bar".to_string())]);

        cmd.set_dedup(id, a(), "Baz".into());
        cmd.apply(&mut world).unwrap();

        assert_eq!(query.collect_vec(&world), [(true, "Baz".to_string())]);

        cmd.set_dedup(id, a(), "Baz".into());
        cmd.apply(&mut world).unwrap();
        assert_eq!(query.collect_vec(&world), [(false, "Baz".to_string())]);
    }
}
