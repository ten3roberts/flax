use std::{
    any::{type_name, TypeId},
    marker::PhantomData,
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{error::Result, CommandBuffer, Error, World};

pub struct ErasedCell<'a> {
    inner: AtomicRefCell<NonNull<u8>>,
    type_id: TypeId,
    _marker: PhantomData<&'a ()>,
}

impl<'a> ErasedCell<'a> {
    pub fn new<T>(value: &'a mut T) -> Self
    where
        T: 'static,
    {
        Self {
            inner: AtomicRefCell::new(NonNull::new(value as *mut T as *mut u8).unwrap()),
            type_id: TypeId::of::<T>(),
            _marker: PhantomData,
        }
    }

    pub fn is<T: 'static>(&self) -> bool {
        self.type_id == TypeId::of::<T>()
    }

    pub fn borrow<T: 'static>(&'a self) -> Result<AtomicRef<'a, T>> {
        if self.is::<T>() {
            let borrow = self
                .inner
                .try_borrow()
                .map_err(|_| Error::Borrow(type_name::<T>()))?;

            Ok(AtomicRef::map(borrow, |v| unsafe {
                v.cast::<T>().as_ref()
            }))
        } else {
            Err(Error::Downcast(type_name::<T>()))
        }
    }

    pub fn borrow_mut<T: 'static>(&'a self) -> Result<AtomicRefMut<'a, T>> {
        if self.is::<T>() {
            let borrow = self
                .inner
                .try_borrow_mut()
                .map_err(|_| Error::BorrowMut(type_name::<T>()))?;

            Ok(AtomicRefMut::map(borrow, |v| unsafe {
                v.cast::<T>().as_mut()
            }))
        } else {
            Err(Error::Downcast(type_name::<T>()))
        }
    }
}

/// Holds external context for system execution.
/// Contains the world and a commandbuffer
pub struct SystemContext<'a> {
    world: AtomicRefCell<&'a mut World>,
    cmd: AtomicRefCell<&'a mut CommandBuffer>,
}

impl<'a> SystemContext<'a> {
    pub fn new(world: &'a mut World, cmd: &'a mut CommandBuffer) -> Self {
        Self {
            world: AtomicRefCell::new(world),
            cmd: AtomicRefCell::new(cmd),
        }
    }

    /// Access the world
    pub fn world(
        &self,
    ) -> std::result::Result<AtomicRef<'_, &'a mut World>, atomic_refcell::BorrowError> {
        self.world.try_borrow()
    }

    /// Access the world mutably
    pub fn world_mut(
        &self,
    ) -> std::result::Result<AtomicRefMut<&'a mut World>, atomic_refcell::BorrowMutError> {
        self.world.try_borrow_mut()
    }

    /// Access the commandbuffer
    pub fn cmp(
        &self,
    ) -> std::result::Result<AtomicRef<'_, &'a mut CommandBuffer>, atomic_refcell::BorrowError>
    {
        self.cmd.try_borrow()
    }

    /// Access the commandbuffer mutably
    pub fn cmd_mut(
        &self,
    ) -> std::result::Result<AtomicRefMut<&'a mut CommandBuffer>, atomic_refcell::BorrowMutError>
    {
        self.cmd.try_borrow_mut()
    }
}
