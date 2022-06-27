use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use atomic_refcell::{AtomicRef, AtomicRefMut};

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
    fn execute(&mut self, ctx: &'w SystemContext<'w>, data: &'w mut Args) -> Ret;
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        // Fallible
        // impl<'w, Func, $($ty,)*T, Err> SystemFn<'w, ($($ty,)*), Result<T, Err>> for Func
        // where
        //     Func: Fn($(<$ty as SystemData<'w>>::Prepared,)*) -> Result<T, Err>,
        //     $($ty: SystemData<'w>,)*
        // {
        //     fn execute<'a>(&mut self, world: &'w World, data: &'w mut ($($ty,)*)) -> Result<T, Err> {
        //         let _prepared = data.prepare_data(world);
        //         (self)($((_prepared.$idx),)*).into()
        //     }
        // }

        // Infallible
        impl<'w, Func, $($ty,)* Ret> SystemFn<'w, ($($ty,)*), Ret> for Func
        where
            Func: for<'x, 'y> FnMut($(<$ty as SystemData<'x, 'y, 'w>>::Data,)*) -> Ret,
            Ret: 'static,
            $($ty: for<'x> SystemData<'x, 'w>,)*
        {
            fn execute<'a>(&mut self, world: &'w SystemContext, data: &'w mut ($($ty,)*)) -> Ret {
                let mut init = <($($ty,)*) as SystemData>::init(world);
                {
                    let data = data.bind(&mut init);
                    (self)($((data.$idx),)*);
                }

                todo!()
            }
        }

        impl<'init, 'w, $($ty,)*> SystemData<'init, 'w> for ($($ty,)*)
        where
            $($ty: SystemData<'init, 'w>,)*
        {
            type Init = ($(<$ty as SystemData<'init, 'w>>::Init,)*);
            type Data = ($(<$ty as SystemData<'init, 'w>>::Data,)*);
            fn init(_ctx: &'w SystemContext) -> Self::Init {
                ($(<$ty>::init(_ctx),)*)
            }

            fn bind(&mut self, init: &'init mut Self::Init) -> Self::Data {
                ($((self.$idx).bind(&mut init.$idx),)*)
            }
        }
    };
}

// tuple_impl! {}
// tuple_impl! { 0 => A }
// tuple_impl! { 0 => A, 1 => B }
// tuple_impl! { 0 => A, 1 => B, 2 => C }
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
    fn get(&mut self, ctx: &'a SystemContext<'a>) -> Self::Data;
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

    fn get(&mut self, ctx: &'a SystemContext<'a>) -> Self::Data {
        Write(ctx.world_mut().expect("Failed to borrow world mutably"))
    }
}

impl<'w, Args, F> SystemFn<'w, Args, ()> for F
where
    Args: for<'x> SystemData<'x>,
    F: FnMut(<Args as SystemData>::Data),
{
    fn execute(&mut self, ctx: &'w SystemContext<'w>, data: &'w mut Args) {
        let data = data.get(ctx);
        (self)(data)
    }
}

#[cfg(test)]
mod test {

    use crate::{system::SystemContext, CommandBuffer, Entity, World};

    use super::{SystemFn, Writable, Write};

    component! {
        name: String,
        health: f32,
    }

    #[test]
    fn system_fn() {
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

        let mut func = &mut spawner;

        (func).execute(&ctx, &mut Writable::<World>::new())
    }
}
