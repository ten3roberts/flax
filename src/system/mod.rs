mod cell;
mod traits;

use std::marker::PhantomData;

use crate::{
    error::{SystemError, SystemResult},
    util::TupleCombine,
    ArchetypeId, Component, ComponentId, Query, QueryData,
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

impl<Args> SystemBuilder<Args> {
    /// Add a new query to the system
    pub fn with<S>(self, other: S) -> SystemBuilder<Args::PushRight>
    where
        S: WorldAccess + for<'x> SystemData<'x>,
        Args: TupleCombine<S>,
    {
        SystemBuilder {
            data: self.data.push_right(other),
        }
    }

    pub fn build<F, Ret>(self, func: F) -> System<F, Args, Ret>
    where
        Args: for<'a> SystemData<'a> + 'static,
        F: for<'a> SystemFn<
            'a,
            (&'a SystemContext<'a>, &'a mut Args),
            <Args as SystemData<'a>>::Data,
            Ret,
        >,
    {
        System::new(func, self.data)
    }
}

/// Holds the data and an inner system satisfying all dependencies
pub struct System<F, Args, Ret> {
    data: Args,
    func: F,
    _marker: PhantomData<Ret>,
}

impl<F, Args, Ret> System<F, Args, Ret>
where
    for<'a> Args: SystemData<'a> + 'a,
    F: for<'a> SystemFn<
        'a,
        (&'a SystemContext<'a>, &'a mut Args),
        <Args as SystemData<'a>>::Data,
        Ret,
    >,
{
    pub fn new(func: F, data: Args) -> Self {
        Self {
            data,
            func,
            _marker: PhantomData,
        }
    }

    /// Convert to a type erased Send + Sync system
    pub fn boxed(self) -> BoxedSystem
    where
        Args: Send + Sync + 'static,
        Ret: Send + Sync + 'static,
        F: Send + Sync + 'static,
        Self: for<'a> SystemFn<'a, &'a SystemContext<'a>, (), SystemResult<()>>,
    {
        BoxedSystem::new(self)
    }
}
impl System<(), (), ()> {
    pub fn builder() -> SystemBuilder<()> {
        SystemBuilder::new()
    }
}

impl<'a, F, Args> SystemFn<'a, &'a SystemContext<'a>, (), SystemResult<()>> for System<F, Args, ()>
where
    Args: SystemData<'a> + 'a,
    F: SystemFn<'a, (&'a SystemContext<'a>, &'a mut Args), Args::Data, ()>,
{
    fn execute(&'a mut self, ctx: &'a SystemContext<'a>) -> SystemResult<()> {
        self.func.execute((ctx, &mut self.data));
        Ok(())
    }
}

impl<'a, F, Args, E> SystemFn<'a, &'a SystemContext<'a>, (), SystemResult<()>>
    for System<F, Args, std::result::Result<(), E>>
where
    Args: SystemData<'a> + 'a,
    F: SystemFn<'a, (&'a SystemContext<'a>, &'a mut Args), Args::Data, std::result::Result<(), E>>,
    E: Into<eyre::Report>,
{
    fn execute(&'a mut self, ctx: &'a SystemContext<'a>) -> SystemResult<()> {
        match self.func.execute((ctx, &mut self.data)) {
            Ok(()) => Ok(()),
            Err(e) => Err(SystemError {
                name: None,
                report: e.into(),
            }),
        }
    }
}

// impl<'w, Data, F, Ty> SystemFn<'w, ()> for System<Data, F, Result<(), Ty>>
// where
//     F: SystemFn<'w, Ty, Data = Data, Output = eyre::Result<()>>,
//     Ty: Into<eyre::Report> + 'static,
//     Data: for<'x> SystemData<'x>,
// {
//     type Data = ();
//     type Output = SystemResult<()>;
//     fn execute(&'w mut self, ctx: &'w SystemContext<'w>, data: &'w mut ()) -> SystemResult<()> {
//         match self.func.execute(ctx, &mut self.data) {
//             Ok(()) => Ok(()),
//             Err(e) => Err(SystemError {
//                 name: None,
//                 report: e.into(),
//             }),
//         }
//     }
// }

// impl<'w, F, D> SystemFn<'w, D> for F
// where
//     F: FnMut(D),
// {
//     type Data = (Query<Component<String>>,);

//     type Output = eyre::Result<()>;

//     fn execute(&'w mut self, ctx: &'w SystemContext<'w>, data: &'w mut Self::Data) -> Self::Output {
//         todo!()
//     }
// }

#[derive(Debug, Clone)]
pub enum Access {
    ArchetypeStorage {
        arch: ArchetypeId,
        component: ComponentId,
        mutable: bool,
    },
}

/// A type erased system
pub struct BoxedSystem {
    system:
        Box<dyn for<'x> SystemFn<'x, &'x SystemContext<'x>, (), SystemResult<()>> + Send + Sync>,
}

impl BoxedSystem {
    pub fn new(
        system: impl for<'x> SystemFn<'x, &'x SystemContext<'x>, (), SystemResult<()>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            system: Box::new(system),
        }
    }

    pub fn execute<'w>(&'w mut self, ctx: &'w SystemContext<'w>) -> SystemResult<()> {
        self.system.execute(ctx)
    }
}

impl<T> From<T> for BoxedSystem
where
    T: for<'x> SystemFn<'x, &'x SystemContext<'x>, (), SystemResult<()>> + Send + Sync + 'static,
{
    fn from(system: T) -> Self {
        Self::new(system)
    }
}

#[cfg(test)]
mod test {

    use crate::{All, CommandBuffer, Component, EntityBuilder, Query, QueryData, World};

    use super::*;

    #[test]
    fn system_builder() {
        component! {
            a: String,
            b: i32,
        };

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(a(), "Foo".to_string())
            .set(b(), 5)
            .spawn(&mut world);

        let mut system = System::builder()
            .with(Query::new(a()))
            // .with(Query::new(b()))
            .build(|mut a: QueryData<Component<String>, All>| {
                assert_eq!(a.prepare().iter().count(), 1)
            });

        let mut fallible = System::builder().with(Query::new(b())).build(
            move |mut query: QueryData<Component<i32>>| -> eyre::Result<()> {
                // Lock archetypes
                let mut query = query.prepare();
                let item: &i32 = query.get(id)?;
                eprintln!("Item: {item}");

                Ok(())
            },
        );

        let mut cmd = CommandBuffer::new();

        let ctx = SystemContext::new(&mut world, &mut cmd);
        system.execute(&ctx).unwrap();

        fallible.execute(&ctx).unwrap();

        world.remove(id, b()).unwrap();

        let mut boxed = fallible.boxed();

        let ctx = SystemContext::new(&mut world, &mut cmd);
        assert!(boxed.execute(&ctx).is_err());
    }
}
