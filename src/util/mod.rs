// Needed in macro expansion
#![allow(unused_parens)]

use core::marker::PhantomData;

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

#[doc(hidden)]
/// A lifetime annotated covariant pointer
pub struct Ptr<'a, T> {
    ptr: *const T,
    _marker: PhantomData<&'a T>,
}

impl<'a, T> Ptr<'a, T> {
    #[inline]
    pub fn new(ptr: *const T) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub unsafe fn add(&self, count: usize) -> Self {
        Self {
            ptr: self.ptr.add(count),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub unsafe fn advance(&mut self, count: usize) {
        self.ptr = self.ptr.add(count);
    }

    #[inline]
    pub unsafe fn as_ref(&self) -> &'a T {
        &*self.ptr
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }
}

unsafe impl<T: Sync> Sync for Ptr<'_, T> {}
unsafe impl<T: Send> Send for Ptr<'_, T> {}

#[doc(hidden)]
/// A lifetime annotated invariant mutable pointer
pub struct PtrMut<'a, T> {
    ptr: *mut T,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T> PtrMut<'a, T> {
    #[inline]
    pub fn new(ptr: *mut T) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub unsafe fn add(&self, count: usize) -> Self {
        Self {
            ptr: self.ptr.add(count),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub unsafe fn advance(&mut self, count: usize) {
        self.ptr = self.ptr.add(count);
    }

    #[inline]
    pub unsafe fn as_mut(&'a mut self) -> &'a mut T {
        &mut *self.ptr
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr
    }
}

unsafe impl<T: Sync> Sync for PtrMut<'_, T> {}
unsafe impl<T: Send> Send for PtrMut<'_, T> {}
