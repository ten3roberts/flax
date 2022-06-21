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
    ($($idx: tt => $ty: ident: $lf: lifetime),*) => {
            // impl<Func, $($ty,)* Ret> SystemFn<($($ty,)*), Ret> for Func
            // where
            //     Func: for<'x> Fn($(<$ty as SystemData<'x>>::Prepared,)*) -> Ret,
            //     $(for<'x> $ty: SystemData<'x>,)*
            // {
            //     fn execute<'a>(&mut self, world: &World, data: &mut ($($ty,)*)) -> Ret {
            //         let _prepared = data.prepare(world);
            //         (self)($((_prepared.$idx),)*)
            //     }
            // }

        impl<'w, $($ty,)*> SystemData<'w> for ($($ty,)*)
        where
            $($ty: SystemData<'w>,)*
        {
            type Prepared = ($(<$ty as SystemData<'w>>::Prepared,)*);
            fn prepare(&'w mut self, _world: &'w World) -> Self::Prepared {
                ($((self.$idx).prepare(_world),)*)
            }
        }
    };
}

tuple_impl! {}
tuple_impl! { 0 => A: 'a }
tuple_impl! { 0 => A: 'a, 1 => B: 'b }
tuple_impl! { 0 => A: 'a, 1 => B: 'b, 2 => C: 'c }
// tuple_impl! { 0 => A: 'a, 1 => B: 'b, 2 => C: 'c, 3 => D: 'd }
// tuple_impl! { 0 => A: 'a, 1 => B: 'b, 2 => C: 'c, 3 => D: 'd, 4 => E: 'e' }
// tuple_impl! { 0 => A: 'a, 1 => B: 'b, 2 => C: 'c, 3 => D: 'd, 4 => E: 'e', 5 => F }
// tuple_impl! { 0 => A: 'a, 1 => B: 'b, 2 => C: 'c, 3 => D: 'd, 4 => E: 'e', 5 => F, 6 => H }
// tuple_impl! { 0 => A: 'a, 1 => B: 'b, 2 => C: 'c, 3 => D: 'd, 4 => E: 'e', 5 => F, 6 => H, 7 => I }
// tuple_impl! { 0 => A: 'a, 1 => B: 'b, 2 => C: 'c, 3 => D: 'd, 4 => E: 'e', 5 => F, 6 => H, 7 => I, 8 => J }

pub trait SystemData<'w> {
    type Prepared;
    fn prepare(&'w mut self, world: &'w World) -> Self::Prepared;
}

impl<'w, Func, A> SystemFn<'w, A, ()> for Func
where
    Func: FnMut(<A as SystemData<'w>>::Prepared),
    A: SystemData<'w>,
{
    fn execute(&mut self, world: &'w World, data: &'w mut A) -> () {
        let prepared = data.prepare(world);
        (self)(prepared)
    }
}

// pub trait FromSystemData<T> {
//     fn from(v: T) -> Self {}
// }
