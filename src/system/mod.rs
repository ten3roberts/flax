mod traits;

use crate::{
    error::{Result, SystemResult},
    util::TupleCombine,
    ArchetypeId, ComponentId, Query, World,
};

pub use traits::*;

pub struct SystemBuilder<T> {
    data: T,
}

impl SystemBuilder<()> {
    /// Creates a new empty system builders.
    pub fn new() -> Self {
        Self { data: () }
    }
}

impl<T> SystemBuilder<T> {
    /// Add a new query to the system
    pub fn with<S>(self, other: S) -> SystemBuilder<T::PushRight>
    where
        S: WorldAccess + for<'x> SystemData<'x>,
        T: TupleCombine<S>,
    {
        SystemBuilder {
            data: self.data.push_right(other),
        }
    }

    pub fn build<F>(self, func: F) -> System<T, F>
    where
        F: SystemFn<T, ()>,
        T: for<'x> SystemData<'x>,
    {
        System {
            data: self.data,
            func,
        }
    }
}

pub struct System<T, F> {
    data: T,
    func: F,
}

impl System<(), ()> {
    pub fn builder() -> SystemBuilder<()> {
        SystemBuilder::new()
    }
}

impl<T, F> SystemFn<(), ()> for System<T, F>
where
    F: SystemFn<T, ()>,
    T: for<'x> SystemData<'x>,
{
    fn execute<'a>(&mut self, world: &World, _: &mut ()) {
        self.func.execute(world, &mut self.data);
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

#[cfg(test)]
mod test {
    use crate::{Fetch, PreparedQuery};

    use super::*;

    #[test]
    fn system_builder() {
        component! {
            a: String,
            b: i32,
        };

        fn handler<T>(a: T) {}

        let system = System::builder()
            .with(Query::new(a()))
            // .with(Query::new(b()))
            .build(|a: PreparedQuery<crate::Component<String>, crate::All>| {});
    }
}
