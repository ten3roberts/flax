use core::{any::TypeId, ptr::NonNull};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

/// Extract a reference from a [`AtomicRefCell`]
/// # Safety
///
/// The returned value must be of the type specified by `ty`
pub unsafe trait ExtractDyn<'a>: Send + Sync {
    /// Dynamically extract a reference of `ty` contained within
    /// # Safety
    ///
    /// The returned pointer is of type `ty` which has a lifetime of `'a`
    unsafe fn extract_dyn(&'a self, ty: TypeId) -> Option<&'a AtomicRefCell<NonNull<()>>>;
}

struct ErasedCell<T: ?Sized> {
    ptr: AtomicRefCell<NonNull<()>>,
    _marker: core::marker::PhantomData<*mut T>,
}

impl<T: ?Sized> ErasedCell<T> {
    unsafe fn new(value: &mut T) -> Self {
        Self {
            ptr: AtomicRefCell::new(NonNull::from(value).cast::<()>()),
            _marker: core::marker::PhantomData,
        }
    }
}

unsafe impl<T: ?Sized> Send for ErasedCell<T> where T: Send {}
unsafe impl<T: ?Sized> Sync for ErasedCell<T> where T: Sync {}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        unsafe impl<'a, $($ty: ?Sized + 'static + Send + Sync,)*> ExtractDyn<'a> for ($(ErasedCell<$ty>,)*) {
            unsafe fn extract_dyn(&'a self, ty: TypeId) -> Option<&'a AtomicRefCell<NonNull<()>>>  {
                $(
                    if TypeId::of::<$ty>() == ty {
                        Some(&self.$idx.ptr)
                    } else
                )*
                {
                    None
                }
            }
        }
    };
}

impl<'a, T> Extract<'a, AtomicRef<'a, T>> for DynInput
where
    T: 'static,
{
    fn extract(&'a self) -> AtomicRef<'a, T> {
        let cell = unsafe { (*self.ptr).extract_dyn(TypeId::of::<T>()) };

        match cell {
            Some(v) => AtomicRef::map(v.borrow(), |v| unsafe { v.cast::<T>().as_ref() }),
            None => panic!("Dynamic input does not contain {}", tynm::type_name::<T>()),
        }
    }
}

impl<'a, T> Extract<'a, Option<AtomicRef<'a, T>>> for DynInput
where
    T: 'static,
{
    fn extract(&'a self) -> Option<AtomicRef<'a, T>> {
        let cell = unsafe { (*self.ptr).extract_dyn(TypeId::of::<T>()) };

        cell.map(|v| AtomicRef::map(v.borrow(), |v| unsafe { v.cast::<T>().as_ref() }))
    }
}

pub struct DynInput {
    ptr: *mut dyn for<'x> ExtractDyn<'x>,
}

impl DynInput {
    unsafe fn new(ptr: *mut dyn for<'x> ExtractDyn<'x>) -> Self {
        Self { ptr }
    }
}

tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_2() {
        let mut a = String::from("Foo");
        let mut b = 5_i32;
        let mut values = unsafe { (ErasedCell::new(&mut a), ErasedCell::new(&mut b)) };
        let input = unsafe { DynInput::new(&mut values as *mut _) };

        unsafe {
            assert_eq!(
                values
                    .extract_dyn(TypeId::of::<String>())
                    .map(|v| v.borrow().cast::<String>().as_ref())
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

    #[test]
    fn extract_dyn() {
        let mut a = String::from("Foo");
        let mut b = 5_i32;
        let mut values = unsafe { (ErasedCell::new(&mut a), ErasedCell::new(&mut b)) };
        let input = unsafe { DynInput::new(&mut values as *mut _) };

        assert_eq!(
            &*<DynInput as Extract<AtomicRef<String>>>::extract(&input),
            &"Foo"
        );
        assert_eq!(
            &*<DynInput as Extract<AtomicRef<i32>>>::extract(&input),
            &5i32
        );
        assert_eq!(
            <DynInput as Extract<Option<AtomicRef<i32>>>>::extract(&input).as_deref(),
            Some(&5i32)
        );

        assert_eq!(
            <DynInput as Extract<Option<AtomicRef<f32>>>>::extract(&input).as_deref(),
            None
        );
    }
}
