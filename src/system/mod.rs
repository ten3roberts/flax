use crate::{
    error::{Result, SystemResult},
    ArchetypeId, ComponentId, Query, TupleCombine, World,
};

pub struct SystemBuilder<T> {
    current: T,
}

impl SystemBuilder<()> {
    /// Creates a new empty system builders.
    pub fn new() -> Self {
        Self { current: () }
    }
}

impl<T> SystemBuilder<T> {
    /// Add a new query to the system
    pub fn with<S>(self, other: S) -> SystemBuilder<T::PushRight>
    where
        S: WorldAccess,
        T: TupleCombine<S>,
    {
        SystemBuilder {
            current: self.current.push_right(other),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Access {
    ArchetypeStorage {
        arch: ArchetypeId,
        component: ComponentId,
        mutable: bool,
    },
}

pub trait WorldAccess {
    /// Returns all the accesses for a system
    fn access(&mut self, world: &World) -> Vec<Access>;
}
