mod cell;
mod traits;

use std::marker::PhantomData;

use crate::{
    error::{SystemError, SystemResult},
    util::TupleCombine,
    ArchetypeId, ComponentId,
};

pub use cell::*;
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
    // Add a new query to the system
    // pub fn with<S>(self, other: S) -> SystemBuilder<T::PushRight>
    // where
    //     S: WorldAccess + for<'x, 'y, 'z> SystemData<'x, 'y>,
    //     T: TupleCombine<S>,
    // {
    //     SystemBuilder {
    //         data: self.data.push_right(other),
    //     }
    // }

    // pub fn build<'w, F, E>(self, func: F) -> System<T, F, E>
    // where
    //     F: SystemFn<'w, T, E>,
    //     E: 'static,
    //     T: for<'x, 'y> SystemData<'x, 'y>,
    // {
    //     System {
    //         data: self.data,
    //         _marker: PhantomData,
    //         func,
    //     }
    // }
}

/// Holds the data and an inner system satisfying all dependencies
pub struct System<D, F, R> {
    data: D,
    func: F,
    _marker: PhantomData<R>,
}

impl System<(), (), ()> {
    pub fn builder() -> SystemBuilder<()> {
        SystemBuilder::new()
    }
}

// impl<'w, T, F, E> SystemFn<'w, (), SystemResult<()>> for System<T, F, Result<(), E>>
// where
//     F: SystemFn<'w, T, Result<(), E>>,
//     E: Into<eyre::Report> + 'static,
//     T: for<'x, 'y> SystemData<'x, 'y>,
// {
//     fn execute<'a>(&'w mut self, ctx: &'w SystemContext, _: &'w mut ()) -> SystemResult<()> {
//         todo!()
//         // match self.func.execute(ctx, &mut self.data) {
//         //     Ok(()) => Ok(()),
//         //     Err(e) => Err(SystemError {
//         //         name: None,
//         //         report: e.into(),
//         //     }),
//         // }
//     }
// }
//
// impl<'w, T, F> SystemFn<'w, (), SystemResult<()>> for System<T, F, ()>
// where
//     F: SystemFn<'w, T, ()>,
//     T: for<'x, 'y> SystemData<'x, 'y>,
// {
//     fn execute<'a>(&'w mut self, ctx: &'w SystemContext, _: &'w mut ()) -> SystemResult<()> {
//         // self.func.execute(ctx, &mut self.data);
//         Ok(())
//     }
// }
//
#[derive(Debug, Clone)]
pub enum Access {
    ArchetypeStorage {
        arch: ArchetypeId,
        component: ComponentId,
        mutable: bool,
    },
}

/// A sized system that is ready to execute on the world.
pub struct BoxedSystem {
    system: Box<dyn for<'x> SystemFn<'x, (), SystemResult<()>> + Send + Sync>,
}

impl BoxedSystem {
    // pub fn new(
    //     system: impl for<'x> SystemFn<'x, (), SystemResult<()>> + Send + Sync + 'static,
    // ) -> Self {
    //     Self {
    //         system: Box::new(system),
    //     }
    // }

    pub fn execute(&mut self, ctx: &SystemContext) -> SystemResult<()> {
        // self.system.execute(ctx, &mut ());
        todo!()
    }
}

// impl<T> From<T> for BoxedSystem
// where
//     T: for<'x> SystemFn<'x, (), SystemResult<()>> + Send + Sync + 'static,
// {
//     fn from(system: T) -> Self {
//         Self::new(system)
//     }
// }

#[cfg(test)]
mod test {

    use crate::{
        error::Result, CommandBuffer, Component, EntityBuilder, PreparedQuery, Query, World,
    };

    use super::*;

    #[test]
    fn system_builder() {
        // component! {
        //     a: String,
        //     b: i32,
        // };
        //
        // let mut world = World::new();
        //
        // let id = EntityBuilder::new()
        //     .set(a(), "Foo".to_string())
        //     .set(b(), 5)
        //     .spawn(&mut world);
        //
        // let mut system: System<_, _, _> = System::builder()
        //     .with(Query::new(a()))
        //     // .with(Query::new(b()))
        //     .build(|mut a: PreparedQuery<_, _>| assert_eq!(a.iter().count(), 1));
        //
        // let mut fallible = System::builder().with(Query::new(b())).build(
        //     |mut query: PreparedQuery<Component<i32>>| -> Result<()> {
        //         let item: &i32 = query.get(id)?;
        //         eprintln!("Item: {item}");
        //
        //         Ok(())
        //     },
        // );
        //
        // let mut cmd = CommandBuffer::new();
        //
        // let ctx = SystemContext::new(&mut world, &mut cmd);
        // system.execute(&ctx, &mut ()).unwrap();
        // fallible.execute(&ctx, &mut ()).unwrap();
        //
        // world.remove(id, b()).unwrap();
        //
        // let ctx = SystemContext::new(&mut world, &mut cmd);
        // assert!(fallible.execute(&ctx, &mut ()).is_err());
    }
}
