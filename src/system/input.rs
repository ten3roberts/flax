use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

/// Describes the ability to extract `T` from `Self`
pub trait Extract<'a, T> {
    /// Returns T from `Self`
    fn extract(&'a self) -> T;
}

impl<'a, T> Extract<'a, &'a T> for T {
    fn extract(&'a self) -> &'a T {
        self
    }
}

/// Extract a reference from a [`AtomicRefCell`]
impl<'a, T> Extract<'a, AtomicRef<'a, T>> for AtomicRefCell<&'a T> {
    fn extract(&'a self) -> AtomicRef<'a, T> {
        AtomicRef::map(self.borrow(), |v| *v)
    }
}

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

/// Extract a reference from a [`AtomicRefCell`]
impl<'a, T> Extract<'a, AtomicRef<'a, T>> for AtomicRefCell<T> {
    fn extract(&'a self) -> AtomicRef<'a, T> {
        self.borrow()
    }
}

/// Extract a reference from a [`AtomicRefCell`]
impl<'a, T> Extract<'a, AtomicRefMut<'a, T>> for AtomicRefCell<T> {
    fn extract(&'a self) -> AtomicRefMut<'a, T> {
        self.borrow_mut()
    }
}
