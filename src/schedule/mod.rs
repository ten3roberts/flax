use itertools::Itertools;

use crate::{error::SystemError, system::BoxedSystem, World};

/// A collection of systems to run on the world
#[derive(Default)]
pub struct Schedule {
    systems: Vec<BoxedSystem>,
}

impl Schedule {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new system to the schedule.
    /// Respects order.
    pub fn with_system(&mut self, system: impl Into<BoxedSystem>) -> &mut Self {
        self.systems.push(system.into());
        self
    }

    /// Execute all systems in the schedule sequentially on the world.
    ///
    /// If a system execution fails, the schedule will proceed and return a list
    /// of all encountered errors
    pub fn execute_seq(&mut self, world: &World) -> std::result::Result<(), Vec<SystemError>> {
        let errors = self
            .systems
            .iter_mut()
            .map(|system| system.execute(world))
            .flat_map(Result::err)
            .collect_vec();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
