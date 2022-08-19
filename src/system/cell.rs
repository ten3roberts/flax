use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{CommandBuffer, World};

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
