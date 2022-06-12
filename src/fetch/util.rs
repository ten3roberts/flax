/// Transform (&T, &U) -> (T, U)
pub trait OwnedTuple {
    type Owned: 'static;

    fn owned(self) -> Self::Owned;
}

impl<T> OwnedTuple for &T
where
    T: Clone + 'static,
{
    type Owned = T;

    fn owned(self) -> Self::Owned {
        (self).clone()
    }
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<$($ty,)*> OwnedTuple for ($($ty,)*)
            where $($ty: OwnedTuple,)*
        {
            type Owned = ($($ty::Owned,)*);
            fn owned(self) -> Self::Owned {
                ($(
                    ( self.$idx ).owned(),
                )*)
            }
        }
    };
}

tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }
