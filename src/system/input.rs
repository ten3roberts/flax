use core::{any::TypeId, ptr::NonNull};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

/// Describes the ability to extract `T` from `Self`
pub trait Extract<'a, T> {
    /// Returns T from `Self`
    fn extract(&'a self) -> T;
}

// impl<'a, T> Extract<'a, &'a T> for T {
//     fn extract(&'a self) -> &'a T {
//         self
//     }
// }

// /// Extract a reference from a [`AtomicRefCell`]
// impl<'a, T> Extract<'a, AtomicRef<'a, T>> for AtomicRefCell<&'a T> {
//     fn extract(&'a self) -> AtomicRef<'a, T> {
//         AtomicRef::map(self.borrow(), |v| *v)
//     }
// }

/// Extract a reference from a [`AtomicRefCell`]
impl<'a, 's, T> Extract<'a, AtomicRef<'a, T>> for AtomicRefCell<&'s mut T> {
    fn extract(&'a self) -> AtomicRef<'a, T> {
        AtomicRef::map(self.borrow(), |v| *v)
    }
}

/// Extract a mutable reference from a [`AtomicRefCell`]
impl<'a, 's, T> Extract<'a, AtomicRefMut<'a, T>> for AtomicRefCell<&'s mut T> {
    fn extract(&'a self) -> AtomicRefMut<'a, T> {
        AtomicRefMut::map(self.borrow_mut(), |v| *v)
    }
}

// /// Extract a reference from a [`AtomicRefCell`]
// impl<'a, T> Extract<'a, AtomicRef<'a, T>> for AtomicRefCell<T> {
//     fn extract(&'a self) -> AtomicRef<'a, T> {
//         self.borrow()
//     }
// }

// /// Extract a reference from a [`AtomicRefCell`]
// impl<'a, T> Extract<'a, AtomicRefMut<'a, T>> for AtomicRefCell<T> {
//     fn extract(&'a self) -> AtomicRefMut<'a, T> {
//         self.borrow_mut()
//     }
// }

/// Extract a reference from a [`AtomicRefCell`]
pub trait ExtractDyn<'a>: Send + Sync {
    /// Dynamically extract a reference of `ty` contained within
    fn extract_dyn(&'a self, ty: TypeId) -> Option<NonNull<()>>;
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'a, $($ty: 'static + Send + Sync,)*> ExtractDyn<'a> for ($($ty,)*) {
            fn extract_dyn(&'a self, ty: TypeId) -> Option<NonNull<()>>  {
                $(
                    if TypeId::of::<$ty>() == ty {
                        Some(NonNull::from(&self.$idx).cast::<()>())
                    } else
                )*
                {
                    None
                }
            }
        }
    };
}

impl<'a, T> Extract<'a, &'a T> for dyn ExtractDyn<'a>
where
    T: 'static,
{
    fn extract(&'a self) -> &'a T {
        match self.extract_dyn(TypeId::of::<T>()) {
            Some(v) => unsafe { v.cast::<T>().as_ref() },
            None => panic!("Dynamic input does not contain {}", tynm::type_name::<T>()),
        }
    }
}

impl<'a, T> Extract<'a, Option<&'a T>> for dyn ExtractDyn<'a>
where
    T: 'static,
{
    fn extract(&'a self) -> Option<&'a T> {
        self.extract_dyn(TypeId::of::<T>())
            .map(|v| unsafe { v.cast::<T>().as_ref() })
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
        let values = ("Foo", 5_i32);

        unsafe {
            assert_eq!(
                values
                    .extract_dyn(TypeId::of::<&str>())
                    .map(|v| v.cast::<&str>().as_ref()),
                Some(&"Foo")
            )
        }

        unsafe {
            assert_eq!(
                values
                    .extract_dyn(TypeId::of::<i32>())
                    .map(|v| v.cast::<i32>().as_ref()),
                Some(&5)
            )
        }

        unsafe {
            assert_eq!(
                values
                    .extract_dyn(TypeId::of::<f32>())
                    .map(|v| v.cast::<f32>().as_ref()),
                None
            )
        }
    }

    #[test]
    fn extract_dyn() {
        let values = ("Foo", 5_i32);
        let values = &values as &dyn ExtractDyn;

        assert_eq!(<dyn ExtractDyn as Extract<&&str>>::extract(values), &"Foo");
        assert_eq!(<dyn ExtractDyn as Extract<&i32>>::extract(values), &5);
        assert_eq!(
            <dyn ExtractDyn as Extract<Option<&i32>>>::extract(values),
            Some(&5)
        );

        assert_eq!(
            <dyn ExtractDyn as Extract<Option<&f32>>>::extract(values),
            None
        );
    }
}
