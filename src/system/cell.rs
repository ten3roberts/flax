use std::{
    any::{type_name, TypeId},
    marker::PhantomData,
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{error::Result, CommandBuffer, Error, World};

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
