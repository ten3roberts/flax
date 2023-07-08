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
pub(crate) struct CellMutGuard<'a, T> {
    value: AtomicRefMut<'a, [T]>,
    // From the refcell
    orig: NonNull<CellData>,
}

unsafe impl<'a, T> Send for CellMutGuard<'a, T> where AtomicRefMut<'a, T>: Send {}
unsafe impl<'a, T> Sync for CellMutGuard<'a, T> where AtomicRefMut<'a, T>: Sync {}

impl<'a, T: ComponentValue> CellMutGuard<'a, T> {
    pub(super) fn new(mut value: AtomicRefMut<'a, CellData>) -> Self {
        // Store the original pointer. This will be used when dropped
        let orig = NonNull::from(&mut *value);

        let value = AtomicRefMut::map(value, |v| v.storage.downcast_mut::<T>());

        Self { value, orig }
    }

    pub(crate) fn set_modified(&mut self, entities: &[Entity], slots: Slice, tick: u32) {
        // SAFETY: `value` is not accessed in this function
        let orig = unsafe { self.orig.as_mut() };

        orig.on_event(EventData {
            ids: &entities[slots.as_range()],
            key: orig.key,
            kind: EventKind::Modified,
        });

        orig.changes
            .set_modified_if_tracking(Change::new(slots, tick));
    }

    pub(crate) fn changes_mut(&mut self) -> &mut Changes {
        // SAFETY: `value` is not accessed in this function
        let orig = unsafe { self.orig.as_mut() };

        &mut orig.changes
    }
}

impl<'w, T> Deref for CellMutGuard<'w, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'w, T> DerefMut for CellMutGuard<'w, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

/// Type safe abstraction over a borrowed cell data
pub(crate) struct CellGuard<'a, T> {
    value: AtomicRef<'a, [T]>,
    orig: NonNull<CellData>,
}

unsafe impl<'a, T> Send for CellGuard<'a, T> where AtomicRef<'a, T>: Send {}
unsafe impl<'a, T> Sync for CellGuard<'a, T> where AtomicRef<'a, T>: Sync {}

impl<'a, T: ComponentValue> CellGuard<'a, T> {
    pub(super) fn new(value: AtomicRef<'a, CellData>) -> Self {
        // Store the original pointer. This will be used when dropped
        let orig = NonNull::from(&*value);

        let value = AtomicRef::map(value, |v| v.storage.downcast_ref::<T>());

        Self { value, orig }
    }

    #[inline]
    pub fn into_inner(self) -> AtomicRef<'a, [T]> {
        self.value
    }

    #[inline]
    pub(crate) fn changes(&self) -> &Changes {
        // SAFETY: `value` is not accessed in this function
        unsafe { &self.orig.as_ref().changes }
    }
}

impl<'w, T> Deref for CellGuard<'w, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

/// A mutable reference to an entity's component with deferred change tracking.
///
/// A modification invent is only generated *if* if this is mutably dereferenced.
pub struct RefMut<'a, T> {
    value: AtomicRefMut<'a, T>,
    // From the refcell
    orig: *mut CellData,

    entities: &'a [Entity],
    slot: Slot,
    modified: bool,
    tick: u32,
}

impl<'a, T: ComponentValue> RefMut<'a, T> {
    pub(super) fn new(
        mut value: AtomicRefMut<'a, CellData>,
        entities: &'a [Entity],
        slot: Slot,
        tick: u32,
    ) -> Option<Self> {
        // Store the original pointer. This will be used when dropped
        let orig = &mut *value as *mut CellData;

        let value =
            AtomicRefMut::filter_map(value, |v| v.storage.downcast_mut::<T>().get_mut(slot))?;

        Some(Self {
            value,
            orig,
            entities,
            slot,
            modified: false,
            tick,
        })
    }
}

impl<'a, T: Debug> Debug for RefMut<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.value.fmt(f)
    }
}

impl<'a, T> Deref for RefMut<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, T> DerefMut for RefMut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.modified = true;
        &mut self.value
    }
}

impl<'a, T> Drop for RefMut<'a, T> {
    #[inline]
    fn drop(&mut self) {
        if self.modified {
            // SAFETY: `value` is not accessed beyond this point
            let orig = unsafe { &mut *self.orig };

            orig.set_modified(self.entities, Slice::single(self.slot), self.tick)
        }
    }
}
