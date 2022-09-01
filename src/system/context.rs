use std::{hash::Hash, sync::Arc};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{Access, AccessKind, CommandBuffer, SystemAccess, SystemData, World};

/// A resource that can be shared between systems
/// The difference between this and an `Arc<Mutex<_>>` is that this will be
/// taken into consideration when multithreading in the schedule, and will as
/// such not require locks.
pub trait SharedResource {
    /// Uniquely identify the access
    fn key(&self) -> u64;
}

impl<'a, T> SystemAccess for Arc<AtomicRefCell<T>>
where
    T: SharedResource,
{
    fn access(&self, _: &World) -> Vec<crate::Access> {
        vec![Access {
            kind: AccessKind::External(self.borrow().key()),
            mutable: true,
        }]
    }
}

impl<'a, T> SystemData<'a> for Arc<AtomicRefCell<T>>
where
    T: 'static + Send + SharedResource,
{
    type Value = AtomicRefMut<'a, T>;

    fn acquire(&'a mut self, _: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        let borrow = self.try_borrow_mut().map_err(|_| {
            eyre::eyre!(
                "Failed to borrow shared resource of {}",
                std::any::type_name::<T>()
            )
        })?;

        Ok(borrow)
    }
}

impl<T> SharedResource for T
where
    T: Send + Hash,
{
    fn key(&self) -> u64 {
        fxhash::hash64(self)
    }
}

/// Holds external context for system execution.
/// Contains the world and a commandbuffer
pub struct SystemContext<'a> {
    world: AtomicRefCell<&'a mut World>,
    cmd: AtomicRefCell<&'a mut CommandBuffer>,
}

impl<'a> SystemContext<'a> {
    /// Creates a new system context
    pub fn new(world: &'a mut World, cmd: &'a mut CommandBuffer) -> Self {
        Self {
            world: AtomicRefCell::new(world),
            cmd: AtomicRefCell::new(cmd),
        }
    }

    /// Access the world
    pub fn world(&self) -> Result<AtomicRef<World>, atomic_refcell::BorrowError> {
        let borrow = self.world.try_borrow()?;
        Ok(AtomicRef::map(borrow, |v| *v))
    }

    /// Access the world mutably
    pub fn world_mut(&self) -> Result<AtomicRefMut<World>, atomic_refcell::BorrowMutError> {
        let borrow = self.world.try_borrow_mut()?;
        Ok(AtomicRefMut::map(borrow, |v| *v))
    }

    /// Access the commandbuffer
    pub fn cmd(&self) -> Result<AtomicRef<CommandBuffer>, atomic_refcell::BorrowError> {
        let borrow = self.cmd.try_borrow()?;
        Ok(AtomicRef::map(borrow, |v| *v))
    }

    /// Access the commandbuffer mutably
    pub fn cmd_mut(&self) -> Result<AtomicRefMut<CommandBuffer>, atomic_refcell::BorrowMutError> {
        let borrow = self.cmd.try_borrow_mut()?;
        Ok(AtomicRefMut::map(borrow, |v| *v))
    }
}
