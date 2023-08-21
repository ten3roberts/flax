use core::{any::TypeId, ops::Deref};

use alloc::{sync::Arc, vec::Vec};
use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{
    system::{Access, AccessKind},
    CommandBuffer, World,
};

use super::{input::ExtractDyn, SystemAccess, SystemData};

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

impl<'a, T> SystemData<'a> for SharedResource<T>
where
    T: Send + 'static,
{
    type Value = AtomicRefMut<'a, T>;

    fn acquire(&'a mut self, _: &'a SystemContext<'_, '_, '_>) -> Self::Value {
        self.borrow_mut()
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SharedResource<")?;
        f.write_str(&tynm::type_name::<T>())?;
        f.write_str(">")
    }
}

/// Everything needed to execute a system
pub struct SystemContext<'w, 'b, 'input> {
    pub(crate) world: AtomicRefCell<&'w mut World>,
    pub(crate) cmd: AtomicRefCell<&'w mut CommandBuffer>,
    /// External input
    input: &'b dyn ExtractDyn<'b, 'input>,
}

impl<'a, 'b, 'input> SystemContext<'a, 'b, 'input> {
    /// Creates a new system context
    pub fn new(
        world: &'a mut World,
        cmd: &'a mut CommandBuffer,
        input: &'b dyn ExtractDyn<'b, 'input>,
    ) -> Self {
        Self {
            world: AtomicRefCell::new(world),
            cmd: AtomicRefCell::new(cmd),
            input,
        }
    }

    /// Access the world
    #[inline]
    pub fn world(&self) -> AtomicRef<World> {
        let borrow = self.world.borrow();
        AtomicRef::map(borrow, |v| *v)
    }

    /// Access the world mutably
    #[inline]
    pub fn world_mut(&self) -> AtomicRefMut<World> {
        let borrow = self.world.borrow_mut();
        AtomicRefMut::map(borrow, |v| *v)
    }

    /// Access the commandbuffer
    #[inline]
    pub fn cmd(&self) -> AtomicRef<CommandBuffer> {
        let borrow = self.cmd.borrow();
        AtomicRef::map(borrow, |v| *v)
    }

    /// Access the commandbuffer mutably
    #[inline]
    pub fn cmd_mut(&self) -> AtomicRefMut<CommandBuffer> {
        let borrow = self.cmd.borrow_mut();
        AtomicRefMut::map(borrow, |v| *v)
    }

    /// Access user provided input data
    #[inline]
    pub fn input<T: 'static>(&self) -> Option<AtomicRef<T>> {
        let cell = unsafe { self.input.extract_dyn(TypeId::of::<T>()) };
        cell.map(|v| AtomicRef::map(v.borrow(), unsafe { |v| v.cast().as_ref() }))
    }

    /// Access user provided input data
    #[inline]
    pub fn input_mut<T: 'static>(&self) -> Option<AtomicRefMut<T>> {
        let cell = unsafe { self.input.extract_dyn(TypeId::of::<T>()) };
        cell.map(|v| AtomicRefMut::map(v.borrow_mut(), unsafe { |v| v.cast().as_mut() }))
    }
}
