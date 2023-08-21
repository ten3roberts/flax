use core::{any::TypeId, ptr::NonNull};

use atomic_refcell::AtomicRefCell;

/// Extract a reference from a [`AtomicRefCell`]
/// # Safety
///
/// The returned value must be of the type specified by `ty`
pub unsafe trait ExtractDyn<'a, 'b>: Send + Sync {
    /// Dynamically extract a reference of `ty` contained within
    /// # Safety
    ///
    /// The returned pointer is of type `ty` which has a lifetime of `'b`
    unsafe fn extract_dyn(&'a self, ty: TypeId) -> Option<&'a AtomicRefCell<NonNull<()>>>;
}

/// Convert a tuple of references into a tuple of AtomicRefCell and type
pub trait IntoInput<'a> {
    /// The type erased cell
    type Output: for<'x> ExtractDyn<'x, 'a>;
    /// # Safety
    ///
    /// The caller must that the returned cell is used with the lifetime of `'a`
    unsafe fn into_input(self) -> Self::Output;
}

unsafe impl<'a, 'b> ExtractDyn<'a, 'b> for () {
    unsafe fn extract_dyn(&'a self, _: TypeId) -> Option<&'a AtomicRefCell<NonNull<()>>> {
        None
    }
}

unsafe impl<'a, 'b, T: 'static + Send + Sync> ExtractDyn<'a, 'b> for ErasedCell<T> {
    #[inline]
    unsafe fn extract_dyn(&'a self, ty: TypeId) -> Option<&'a AtomicRefCell<NonNull<()>>> {
        if TypeId::of::<T>() == ty {
            Some(&self.cell)
        } else {
            None
        }
    }
}

unsafe impl<'a, T: 'static + Send + Sync> IntoInput<'a> for &'a mut T {
    type Output = ErasedCell<T>;
    fn into_input(self) -> Self::Output {
        unsafe { ErasedCell::new(self) }
    }
}

pub struct ErasedCell<T: ?Sized> {
    cell: AtomicRefCell<NonNull<()>>,
    _marker: core::marker::PhantomData<*mut T>,
}

impl<T: ?Sized> ErasedCell<T> {
    unsafe fn new(value: &mut T) -> Self {
        Self {
            cell: AtomicRefCell::new(NonNull::from(value).cast::<()>()),
            _marker: core::marker::PhantomData,
        }
    }
}

unsafe impl<T: ?Sized> Send for ErasedCell<T> where T: Send {}
unsafe impl<T: ?Sized> Sync for ErasedCell<T> where T: Sync {}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        unsafe impl<'a, $($ty: ?Sized + 'static + Send + Sync,)*> IntoInput<'a> for ($(&'a mut $ty,)*) {
            type Output = ($(ErasedCell<$ty>,)*);

            fn into_input(self) -> Self::Output {
                unsafe { ($(ErasedCell::new(self.$idx),)*) }
            }
        }

        unsafe impl<'a, 'b, $($ty: ?Sized + 'static + Send + Sync,)*> ExtractDyn<'a , 'b> for ($(ErasedCell<$ty>,)*) {
            unsafe fn extract_dyn(&'a self, ty: TypeId) -> Option<&'a AtomicRefCell<NonNull<()>>>  {
                $(
                    if TypeId::of::<$ty>() == ty {
                        Some(&self.$idx.cell)
                    } else
                )*
                {
                    None
                }
            }
        }
    };
}

tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    #[test]
    fn extract_2() {
        let mut a = String::from("Foo");
        let mut b = 5_i32;
        let values = unsafe { (ErasedCell::new(&mut a), ErasedCell::new(&mut b)) };

        unsafe {
            assert_eq!(
                values
                    .extract_dyn(TypeId::of::<String>())
                    .map(|v| v.borrow().cast::<alloc::string::String>().as_ref())
                    .map(|v| &**v),
                Some("Foo")
            );

            assert_eq!(
                values
                    .extract_dyn(TypeId::of::<i32>())
                    .map(|v| v.borrow().cast::<i32>().as_ref()),
                Some(&5)
            );

            assert_eq!(
                values
                    .extract_dyn(TypeId::of::<f32>())
                    .map(|v| v.borrow().cast::<f32>().as_ref()),
                None
            )
        }
    }
}
