use std::collections::{btree_map::Entry, BTreeMap};

use itertools::Itertools;

use crate::{
    error::Result, BatchSpawn, BufferStorage, Component, ComponentInfo, ComponentValue, Entity,
    EntityBuilder, World,
};

enum Spawn {
    One(EntityBuilder),
    Batch(BatchSpawn),
}

type DeferFn = Box<dyn Fn(&mut World) -> eyre::Result<()> + Send + Sync>;

/// Records commands into the world.
/// Allows insertion and removal of components when the world is not available
/// mutably, such as in systems or during iteration.
#[derive(Default)]
pub struct CommandBuffer {
    inserts: BufferStorage,
    insert_locations: BTreeMap<(Entity, ComponentInfo), usize>,
    spawned: Vec<Spawn>,
    despawned: Vec<Entity>,
    removals: Vec<(Entity, ComponentInfo)>,
    defers: Vec<DeferFn>,
}

impl std::fmt::Debug for CommandBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandBuffer")
            .field("inserts", &self.insert_locations.len())
            .field("spawned", &self.spawned.len())
            .field("despawned", &self.despawned)
            .field("removals", &self.removals)
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
    /// Unlike, [`World::insert`] it does not return the old value as that is
    /// not known at call time.
    pub fn set<T: ComponentValue>(
        &mut self,
        id: impl Into<Entity>,
        component: Component<T>,
        value: T,
    ) -> &mut Self {
        match self.insert_locations.entry((id.into(), component.info())) {
            Entry::Vacant(slot) => {
                let offset = self.inserts.insert(value);
                slot.insert(offset);
            }
            Entry::Occupied(slot) => unsafe {
                self.inserts.swap(*slot.get(), value);
            },
        }

        self
    }

    /// Deferred removal of a component for `id`.
    /// Unlike, [`World::remove`] it does not return the old value as that is
    /// not known at call time.
    pub fn remove<T: ComponentValue>(
        &mut self,
        id: impl Into<Entity>,
        component: Component<T>,
    ) -> &mut Self {
        let id = id.into();
        let offset = self.insert_locations.remove(&(id, component.info()));

        // Remove from insert list
        if let Some(offset) = offset {
            unsafe { self.inserts.take::<T>(offset) };
        }

        self.removals.push((id, component.info()));

        self
    }

    /// Spawn a new entity with the given components of the builder
    pub fn spawn(&mut self, entity: impl Into<EntityBuilder>) -> &mut Self {
        self.spawned.push(Spawn::One(entity.into()));

        self
    }

    /// Spawn a new batch with the given components of the builder
    pub fn spawn_batch(&mut self, batch: BatchSpawn) -> &mut Self {
        self.spawned.push(Spawn::Batch(batch));

        self
    }
    /// Despawn an entity by id
    pub fn despawn(&mut self, id: Entity) -> &mut Self {
        // // Drop all inserts for this component
        // self.insert_locations
        //     .iter()
        //     .skip_while(|((entity, _), _)| *entity != id)
        //     .take_while(|((entity, _), _)| *entity == id)
        //     .for_each(|((_, component), offset)| unsafe {
        //         eprintln!("Removing insert for despawned entity");
        //         let ptr = self.inserts.take_dyn(*offset);
        //         (component.drop)(ptr);
        //     });

        // self.removals.retain(|(entity, _)| *entity != id);

        self.despawned.push(id);
        self
    }

    /// Defer a function to execute upon the world.
    ///
    /// Errors will be propagated.
    pub fn defer(
        &mut self,
        func: impl Fn(&mut World) -> eyre::Result<()> + Send + Sync + 'static,
    ) -> &mut Self {
        self.defers.push(Box::new(func));
        self
    }

    /// Applies all contents of the command buffer to the world.
    /// The commandbuffer is cleared and can be reused.
    #[tracing::instrument(skip(world))]
    pub fn apply(&mut self, world: &mut World) -> eyre::Result<()> {
        let groups = self
            .insert_locations
            .iter()
            .group_by(|((entity, _), _)| *entity);

        let storage = &mut self.inserts;

        (&groups).into_iter().try_for_each(|(id, group)| {
            // Safety
            // The offset is acquired from the map which was previously acquired
            unsafe {
                let components =
                    group.map(|((_, info), offset)| (*info, storage.take_dyn(*offset)));
                world.set_with(id, components)
            }
        })?;

        self.spawned.drain(..).for_each(|spawn| match spawn {
            Spawn::One(mut builder) => {
                builder.spawn(world);
            }
            Spawn::Batch(mut batch) => {
                batch.spawn(world);
            }
        });

        self.removals
            .drain(..)
            .try_for_each(|(id, component)| world.remove_dyn(id, component))?;

        self.despawned
            .drain(..)
            .try_for_each(|id| world.despawn(id))?;

        self.defers.drain(..).try_for_each(|func| (func)(world))?;

        self.clear();
        Ok(())
    }

    /// Clears all values in the component buffer but keeps allocations around.
    /// Is automatically called for [`Self::apply`].
    pub fn clear(&mut self) {
        self.inserts.clear();
        self.insert_locations.clear();
        self.removals.clear();
        self.despawned.clear();
        self.spawned.clear();
    }
}
