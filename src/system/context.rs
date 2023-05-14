use core::{any::TypeId, ops::Deref};

use alloc::{sync::Arc, vec::Vec};
use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{
    system::{Access, AccessKind},
    CommandBuffer, World,
};

use super::{SystemAccess, SystemData};

/// A resource that can be shared between systems
/// The difference between this and an `Arc<Mutex<_>>` is that this will be
/// taken into consideration when multithreading in the schedule, and will as
/// such not require locks.
///
/// The implementation is an `Arc<AtomicRefCell>` and is thus cheap to clone
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SharedResource<T>(Arc<AtomicRefCell<T>>);

impl<T: Send + 'static> SharedResource<T> {
    /// Creates a new shared resource
    pub fn new(value: T) -> Self {
        Self(Arc::new(AtomicRefCell::new(value)))
    }
}

impl<T> Deref for SharedResource<T> {
    type Target = AtomicRefCell<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// pub trait SharedResource: SystemAccess + for<'x> SystemData<'x> {}

// impl<T: Send + Sync + 'static> SharedResource for Arc<AtomicRefCell<T>> {}

impl<T> SystemAccess for SharedResource<T>
where
    T: Send + 'static,
{
    fn access(&self, _: &World) -> Vec<Access> {
        alloc::vec![Access {
            kind: AccessKind::External(TypeId::of::<Self>()),
            mutable: true,
        }]
    }
}

impl<'a, T> SystemData<'a> for SharedResource<T>
where
    T: Send + 'static,
{
    type Value = AtomicRefMut<'a, T>;

    fn acquire(&'a mut self, _: &'a SystemContext<'_>) -> anyhow::Result<Self::Value> {
        let borrow = self.try_borrow_mut().map_err(|_| {
            anyhow::anyhow!(
                "Failed to borrow shared resource of {}",
                core::any::type_name::<T>()
            )
        })?;

        Ok(borrow)
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SharedResource<")?;
        f.write_str(&tynm::type_name::<T>())?;
        f.write_str(">")
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
