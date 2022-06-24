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
    Args: for<'x> SystemData<'x, 'w>,
    Ret: 'static,
{
    fn execute(&'w mut self, ctx: &'w SystemContext<'w>, data: &'w mut Args) -> Ret;
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
            Func: for<'x> FnMut($(<$ty as SystemData<'x, 'w>>::Data,)*) -> Ret,
            Ret: 'static,
            $($ty: for<'x> SystemData<'x, 'w>,)*
        {
            fn execute<'a>(&mut self, world: &'w SystemContext<'w>, data: &'w mut ($($ty,)*)) -> Ret {
                let init = <($($ty,)*) as SystemData>::init(world);
                {
                    let data = data.bind(&init);
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

            fn bind(&mut self, init: &Self::Init) -> Self::Data {
                ($((self.$idx).bind(&init.$idx),)*)
            }
        }
    };
}

tuple_impl! {}
tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }

pub trait SystemData<'init, 'ctx> {
    type Init;
    type Data;
    /// Initialize and fetch data from the system execution context
    fn init(ctx: &'ctx SystemContext) -> Self::Init;
    /// Bind the data to a value passed into the system.
    ///
    /// The two stage process is required to infer an appropriate lifetime for
    /// the `AtomicRef` from the context, and the reference held within
    fn bind(&mut self, init: &Self::Init) -> Self::Data;
}
