// Needed in macro expansion
#![allow(unused_parens)]

mod cloned;
mod fn_args;
pub use cloned::*;

pub trait TupleCombine<T> {
    type PushRight;
    type PushLeft;

    fn push_right(self, value: T) -> Self::PushRight;
    fn push_left(self, value: T) -> Self::PushLeft;
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<$($ty,)*> TupleCloned for ($($ty,)*)
            where $($ty: TupleCloned,)*
        {
            type Cloned = ($($ty::Cloned,)*);
            fn cloned(self) -> Self::Cloned {
                ($(
                    ( self.$idx ).cloned(),
                )*)
            }
        }

        impl<$($ty,)* T> TupleCombine<T> for ($($ty,)*) {
            type PushRight = ($($ty,)* T);
            type PushLeft  = (T, $($ty,)*);
            fn push_right(self, value: T) -> Self::PushRight {
                ($(
                    ( self.$idx ),
                )* value)
            }

            fn push_left(self, value: T) -> Self::PushLeft {
                (value, $(
                    ( self.$idx ),
                )*)
            }
        }
    };
}

impl TupleCloned for () {
    type Cloned = ();

    fn cloned(self) -> Self::Cloned {}
}

impl<T> TupleCombine<T> for () {
    type PushRight = (T,);

    type PushLeft = (T,);

    fn push_right(self, value: T) -> Self::PushRight {
        (value,)
    }

    fn push_left(self, value: T) -> Self::PushLeft {
        (value,)
    }
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

#[cfg(test)]
mod test {
    use super::{TupleCloned, TupleCombine};

    #[test]
    fn tuples_clone() {
        let a: i32 = 5;
        let b = "foo".to_string();

        let tuple = (&a, &b);

        let cloned: (i32, String, i32) = tuple.push_right(&a).cloned();
        assert_eq!(cloned, (a, b, a));
        let t = ().push_right(5);
        assert_eq!(t, (5,));
    }
}
