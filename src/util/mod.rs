// Needed in macro expansion
#![allow(unused_parens)]

use crate::filter::All;

/// Allows pushing onto a tuple
pub trait TupleCombine<T> {
    /// The resulting right push
    type PushRight;
    /// The resulting left push
    type PushLeft;

    /// Pushes `T` from the right
    fn push_right(self, value: T) -> Self::PushRight;
    /// Pushes `T` from the left
    fn push_left(self, value: T) -> Self::PushLeft;
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
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

    use super::TupleCombine;

    #[test]
    fn tuples_push() {
        let a: i32 = 5;
        let b = "foo";

        let tuple = (a, b);

        let cloned: (i32, &str, i64) = tuple.push_right(a as i64);
        assert_eq!(cloned, (a, b, a as i64));
        let t = ().push_right(5);
        assert_eq!(t, (5,));
    }
}

impl<T> TupleCombine<T> for All {
    type PushRight = (All, T);

    type PushLeft = (T, All);

    fn push_right(self, value: T) -> Self::PushRight {
        (self, value)
    }

    fn push_left(self, value: T) -> Self::PushLeft {
        (value, self)
    }
}
