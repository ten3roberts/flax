use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefMut};

use crate::{component::ComponentValue, Entity};

use super::{CellData, Changes, Slice, Slot};

/// Type safe abstraction over a borrowed cell data
pub(crate) struct CellMutGuard<'a, T: ?Sized> {
    data: AtomicRefMut<'a, CellData>,
    // From the refcell
    storage: NonNull<T>,
}

unsafe impl<'a, T: 'a + ?Sized> Send for CellMutGuard<'a, T> where for<'x> &'x mut T: Send {}
unsafe impl<'a, T: 'a + ?Sized> Sync for CellMutGuard<'a, T> where for<'x> &'x mut T: Sync {}

impl<'a, T: ComponentValue + Sized> CellMutGuard<'a, [T]> {
    pub(super) fn new(mut value: AtomicRefMut<'a, CellData>) -> Self {
        let storage: NonNull<[T]> = NonNull::from(value.storage.downcast_mut::<T>());

        Self {
            data: value,
            storage,
        }
    }
}

impl<'a, T: ?Sized> CellMutGuard<'a, T> {
    pub(crate) fn set_modified(&mut self, ids: &[Entity], slots: Slice, tick: u32) {
        // SAFETY: `value` is not accessed in this function
        let data = &mut *self.data;
        data.set_modified(ids, slots, tick)
    }

    pub(crate) fn filter_map<U>(
        mut self,
        f: impl FnOnce(&mut T) -> Option<&mut U>,
    ) -> Option<CellMutGuard<'a, U>> {
        let storage = NonNull::from(f(unsafe { self.storage.as_mut() })?);

        Some(CellMutGuard {
            data: self.data,
            storage,
        })
    }

    pub(crate) fn storage(&self) -> NonNull<T> {
        self.storage
    }

    pub(crate) fn get(&self) -> &T {
        unsafe { self.storage.as_ref() }
    }

    pub(crate) fn get_mut(&mut self) -> &mut T {
        unsafe { self.storage.as_mut() }
    }
}

impl<'a, T: Debug + ?Sized> Debug for CellMutGuard<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (*self.get()).fmt(f)
    }
}

/// Type safe abstraction over a borrowed cell data
pub(crate) struct CellGuard<'a, T: ?Sized> {
    data: AtomicRef<'a, CellData>,
    storage: NonNull<T>,
}

unsafe impl<'a, T: 'a + ?Sized> Send for CellGuard<'a, T> where for<'x> &'x T: Send {}
unsafe impl<'a, T: 'a + ?Sized> Sync for CellGuard<'a, T> where for<'x> &'x T: Sync {}

impl<'a, T: ComponentValue + Sized> CellGuard<'a, [T]> {
    pub(super) fn new(value: AtomicRef<'a, CellData>) -> Self {
        let storage: NonNull<[T]> = NonNull::from(value.storage.downcast_ref::<T>());

        Self {
            data: value,
            storage,
        }
    }
}

impl<'a, T: ?Sized> CellGuard<'a, T> {
    #[inline]
    pub(crate) fn into_inner(self) -> AtomicRef<'a, T> {
        AtomicRef::map(self.data, |_| unsafe { self.storage.as_ref() })
    }

    #[inline]
    pub(crate) fn changes(&self) -> &Changes {
        &self.data.changes
    }

    #[inline]
    pub(crate) fn get(&self) -> &T {
        unsafe { self.storage.as_ref() }
    }
}

impl<'a, T: Debug + ?Sized> Debug for CellGuard<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (*self.get()).fmt(f)
    }
}

/// A mutable reference to an entity's component with deferred change tracking.
///
/// A modification invent is only generated *if* if this is mutably dereferenced.
pub struct RefMut<'a, T> {
    guard: CellMutGuard<'a, T>,

    id: Entity,
    slot: Slot,
    modified: bool,
    tick: u32,
}

impl<'a, T: ComponentValue> RefMut<'a, T> {
    pub(super) fn new(
        guard: CellMutGuard<'a, [T]>,
        id: Entity,
        slot: Slot,
        tick: u32,
    ) -> Option<Self> {
        // Store the original pointer. This will be used when dropped
        let guard = guard.filter_map(|v| v.get_mut(slot))?;

        Some(Self {
            guard,
            id,
            slot,
            modified: false,
            tick,
        })
    }
}

impl<'a, T: Debug> Debug for RefMut<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.guard.fmt(f)
    }
}

impl<'a, T> Deref for RefMut<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.guard.get()
    }
}

impl<'a, T> DerefMut for RefMut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.modified = true;
        self.guard.get_mut()
    }
}

impl<'a, T> Drop for RefMut<'a, T> {
    #[inline]
    fn drop(&mut self) {
        if self.modified {
            // SAFETY: `value` is not accessed beyond this point
            self.guard
                .data
                .set_modified(&[self.id], Slice::single(self.slot), self.tick)
        }
    }
}
