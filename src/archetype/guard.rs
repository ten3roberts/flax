use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefMut};

use crate::{
    events::{EventData, EventKind},
    ComponentValue, Entity,
};

use super::{CellData, Change, Changes, Slice, Slot};

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
    pub(crate) fn set_modified(&mut self, entities: &[Entity], slots: Slice, tick: u32) {
        // SAFETY: `value` is not accessed in this function
        let data = &mut *self.data;
        data.on_event(EventData {
            ids: &entities[slots.as_range()],
            key: data.key,
            kind: EventKind::Modified,
        });

        data.changes
            .set_modified_if_tracking(Change::new(slots, tick));
    }

    pub(crate) fn changes_mut(&mut self) -> &mut Changes {
        // SAFETY: `value` is not accessed in this function

        &mut self.data.changes
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
}

impl<'w, T: ?Sized> Deref for CellMutGuard<'w, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.storage.as_ref() }
    }
}

impl<'w, T: ?Sized> DerefMut for CellMutGuard<'w, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.storage.as_mut() }
    }
}

impl<'a, T: Debug + ?Sized> Debug for CellMutGuard<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
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
    pub fn into_slice_ref(self) -> AtomicRef<'a, T> {
        AtomicRef::map(self.data, |_| unsafe { self.storage.as_ref() })
    }

    pub(crate) fn filter_map<U>(
        self,
        f: impl FnOnce(&T) -> Option<&U>,
    ) -> Option<CellGuard<'a, U>> {
        let storage = NonNull::from(f(unsafe { self.storage.as_ref() })?);

        Some(CellGuard {
            data: self.data,
            storage,
        })
    }

    #[inline]
    pub(crate) fn changes(&self) -> &Changes {
        &self.data.changes
    }
}

impl<'a, T: Debug + ?Sized> Debug for CellGuard<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

impl<'w, T: ?Sized> Deref for CellGuard<'w, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.storage.as_ref() }
    }
}

/// A mutable reference to an entity's component with deferred change tracking.
///
/// A modification invent is only generated *if* if this is mutably dereferenced.
pub struct RefMut<'a, T> {
    guard: CellMutGuard<'a, T>,

    // All entities in the archetype
    ids: &'a [Entity],
    slot: Slot,
    modified: bool,
    tick: u32,
}

impl<'a, T: ComponentValue> RefMut<'a, T> {
    pub(super) fn new(
        guard: CellMutGuard<'a, [T]>,
        ids: &'a [Entity],
        slot: Slot,
        tick: u32,
    ) -> Option<Self> {
        // Store the original pointer. This will be used when dropped
        let guard = guard.filter_map(|v| v.get_mut(slot))?;

        Some(Self {
            guard,
            ids,
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
        &self.guard
    }
}

impl<'a, T> DerefMut for RefMut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.modified = true;
        &mut self.guard
    }
}

impl<'a, T> Drop for RefMut<'a, T> {
    #[inline]
    fn drop(&mut self) {
        if self.modified {
            // SAFETY: `value` is not accessed beyond this point
            self.guard.data.set_modified(
                &self.ids[self.slot..=self.slot],
                Slice::single(self.slot),
                self.tick,
            )
        }
    }
}
