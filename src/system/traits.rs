use crate::World;

use super::Access;

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
    Args: SystemData<'w>,
{
    fn execute(&'w mut self, world: &'w World, data: &'w mut Args) -> Ret;
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
            Func: FnMut($(<$ty as SystemData<'w>>::Prepared,)*) -> Ret,
            $($ty: SystemData<'w>,)*
        {
            fn execute<'a>(&mut self, world: &'w World, data: &'w mut ($($ty,)*)) -> Ret {
                let _prepared = data.prepare_data(world);
                (self)($((_prepared.$idx),)*)
            }
        }

        impl<'w, $($ty,)*> SystemData<'w> for ($($ty,)*)
        where
            $($ty: SystemData<'w>,)*
        {
            type Prepared = ($(<$ty as SystemData<'w>>::Prepared,)*);
            fn prepare_data(&'w mut self, _world: &'w World) -> Self::Prepared {
                ($((self.$idx).prepare_data(_world),)*)
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
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }

pub trait SystemData<'w> {
    type Prepared;
    fn prepare_data(&'w mut self, world: &'w World) -> Self::Prepared;
}
