use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use eyre::eyre;

use crate::World;

use super::{cell::SystemContext, Access};

/// Describe an access to the world in ters of shared and unique accesses
pub trait WorldAccess {
    /// Returns all the accesses for a system
    fn access(&mut self, world: &World) -> Vec<Access>;
}

/// Describes a type which can run on a set of system data.
///
/// Is implemented for functions up to an arity of 8
pub trait SystemFn<'w, Args, Ret>
where
    Ret: 'static,
{
    fn execute(&'w mut self, ctx: &'w SystemContext<'w>, data: &'w mut Args) -> Ret;
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        // Fallible
        impl<'w, Func, $($ty,)* T> SystemFn<'w, ($($ty,)*), eyre::Result<T>> for Func
        where
            Func: FnMut($(<$ty as SystemData>::Data,)*) -> eyre::Result<T>,
            T: 'static,
            $($ty: for<'x> SystemData<'x>,)*
        {
            fn execute(&'w mut self, ctx: &'w SystemContext<'w>, data: &'w mut ($($ty,)*)) -> eyre::Result<T> {
                let _data = data.get(ctx)?;
                (self)($((_data.$idx),)*)
            }
        }

        // Infallible
        impl<'w, Func, $($ty,)*> SystemFn<'w, ($($ty,)*), ()> for Func
        where
            Func: FnMut($(<$ty as SystemData>::Data,)*),
            $($ty: for<'x> SystemData<'x>,)*
        {
            fn execute(&'w mut self, ctx: &'w SystemContext<'w>, data: &'w mut ($($ty,)*)) {
                let _data = data.get(ctx).expect("Failed to get system data from context");
                (self)($((_data.$idx),)*)
            }
        }
        impl<'w, $($ty,)*> SystemData<'w> for ($($ty,)*)
        where
            $($ty: SystemData<'w>,)*
        {
            type Data = ($(<$ty as SystemData<'w>>::Data,)*);

            fn get(&'w mut self, _ctx: &'w SystemContext<'w>) -> eyre::Result<Self::Data> {
                Ok(
                    ($((self.$idx).get(_ctx)?,)*)
                )
            }
        }
    };
}

tuple_impl! {}
tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }

// pub trait SystemData<'init, 'ctx, 'w> {
//     type Init;
//     /// Initialize and fetch data from the system execution context
//     fn init(ctx: &'ctx SystemContext<'w>) -> Self::Init;
// }

/// Describes a type which can fetch assocated Data from the system context and
/// provide it to the system.
pub trait SystemData<'a> {
    type Data;
    fn get(&'a mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<Self::Data>;
}

pub struct Writable<T>(PhantomData<T>);
pub struct Write<'a, T>(AtomicRefMut<'a, &'a mut T>);
#[derive(Debug)]
pub struct Read<'a, T>(AtomicRef<'a, &'a mut T>);

impl<'a, T> Deref for Read<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<'a, T> Deref for Write<'a, T> {
    type Target = &'a mut T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<'a, T> DerefMut for Write<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<T> Writable<T> {
    fn new() -> Self {
        Self(PhantomData)
    }
}

impl<'a> SystemData<'a> for Writable<World> {
    type Data = Write<'a, World>;

    fn get(&mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<Self::Data> {
        Ok(Write(
            ctx.world_mut()
                .map_err(|_| eyre!("Failed to borrow world mutably"))?,
        ))
    }
}

// impl<'w, Args, F> SystemFn<'w, Args, eyre::Result<()>> for F
// where
//     Args: for<'x> SystemData<'x>,
//     F: FnMut(<Args as SystemData>::Data),
// {
//     fn execute(&mut self, ctx: &'w SystemContext<'w>, data: &'w mut Args) -> eyre::Result<()> {
//         let data = data.get(ctx)?;
//         (self)(data);
//         Ok(())
//     }
// }

#[cfg(test)]
mod test {

    use itertools::Itertools;

    use crate::{system::SystemContext, CommandBuffer, Entity, Query, QueryData, World};

    use super::{SystemFn, Writable, Write};

    component! {
        name: String,
        health: f32,
    }

    #[test]
    fn system_fn() -> eyre::Result<()> {
        let mut world = World::new();
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(&mut world, &mut cmd);

        let mut spawner = |mut w: Write<_>| {
            Entity::builder()
                .set(name(), "Neo".to_string())
                .set(health(), 90.0)
                .spawn(*w);

            Entity::builder()
                .set(name(), "Trinity".to_string())
                .set(health(), 85.0)
                .spawn(*w);
        };

        let mut reader = |mut q: QueryData<_, _>| {
            let names = q.prepare().iter().cloned().sorted().collect_vec();

            assert_eq!(names, ["Neo", "Trinity"]);
        };

        let data = &mut (Writable::<World>::new(),);
        let mut spawner = &mut spawner;
        let mut reader = &mut reader;

        (spawner).execute(&*&ctx, data);

        let data = &mut (Query::new(name()),);
        (reader).execute(&*&ctx, data);
        Ok(())
    }
}
