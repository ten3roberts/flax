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
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::External(TypeId::of::<Self>()),
            mutable: true,
        });
    }
}

impl<'a, T, D> SystemData<'a, D> for SharedResource<T>
where
    T: Send + 'static,
{
    type Value = AtomicRefMut<'a, T>;

    fn acquire(&'a mut self, _: &'a SystemContext<'_, D>) -> Self::Value {
        self.borrow_mut()
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SharedResource<")?;
        f.write_str(&tynm::type_name::<T>())?;
        f.write_str(">")
    }
}

/// Holds external context for system execution.
/// Contains the world and a commandbuffer
pub struct SystemContext<'a, T> {
    world: AtomicRefCell<&'a mut World>,
    cmd: AtomicRefCell<&'a mut CommandBuffer>,
    /// User supplied
    data: AtomicRefCell<&'a mut T>,
}

impl<'a, T> SystemContext<'a, T> {
    /// Creates a new system context
    pub fn new(world: &'a mut World, cmd: &'a mut CommandBuffer, data: &'a mut T) -> Self {
        Self {
            world: AtomicRefCell::new(world),
            cmd: AtomicRefCell::new(cmd),
            data: AtomicRefCell::new(data),
        }
    }

    /// Access the world
    pub fn world(&self) -> AtomicRef<World> {
        let borrow = self.world.borrow();
        AtomicRef::map(borrow, |v| *v)
    }

    /// Access the world mutably
    pub fn world_mut(&self) -> AtomicRefMut<World> {
        let borrow = self.world.borrow_mut();
        AtomicRefMut::map(borrow, |v| *v)
    }

    /// Access the commandbuffer
    pub fn cmd(&self) -> AtomicRef<CommandBuffer> {
        let borrow = self.cmd.borrow();
        AtomicRef::map(borrow, |v| *v)
    }

    /// Access the commandbuffer mutably
    pub fn cmd_mut(&self) -> AtomicRefMut<CommandBuffer> {
        let borrow = self.cmd.borrow_mut();
        AtomicRefMut::map(borrow, |v| *v)
    }

    /// Access user provided context data
    pub fn data(&self) -> AtomicRef<T> {
        let borrow = self.data.borrow();
        AtomicRef::map(borrow, |v| *v)
    }

    /// Access user provided context data mutably
    pub fn data_mut(&self) -> AtomicRefMut<T> {
        let borrow = self.data.borrow_mut();
        AtomicRefMut::map(borrow, |v| *v)
    }
}
