use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use atomic_refcell::AtomicRefMut;

use crate::{
    events::{EventData, EventKind},
    Entity,
};

use super::{Cell, Change, Changes, Slice, Slot};

pub(crate) struct CellMutGuard<'a, T: ?Sized> {
    pub(crate) storage: AtomicRefMut<'a, T>,
    pub(crate) changes: AtomicRefMut<'a, Changes>,
    pub(crate) cell: &'a Cell,
    pub(crate) ids: &'a [Entity],
    pub(crate) tick: u32,
}

impl<'a, T: Debug + ?Sized> Debug for CellMutGuard<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.storage.fmt(f)
    }
}

impl<'a, T: ?Sized> CellMutGuard<'a, T> {
    pub(crate) fn set_modified(&mut self, slots: Slice) {
        let event = EventData {
            ids: &self.ids[slots.as_range()],
            key: self.cell.info.key,
            kind: EventKind::Modified,
        };

        for handler in self.cell.subscribers.iter() {
            handler.on_event(&event)
        }

        self.changes
            .set_modified_if_tracking(Change::new(slots, self.tick));
    }

    #[inline]
    pub(crate) fn filter_map<F, U>(self, func: F) -> Option<CellMutGuard<'a, U>>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
    {
        let storage = AtomicRefMut::filter_map(self.storage, func)?;
        Some(CellMutGuard {
            storage,
            changes: self.changes,
            cell: self.cell,
            ids: self.ids,
            tick: self.tick,
        })
    }

    #[inline]
    pub(crate) fn get(&self) -> &T {
        &self.storage
    }

    #[inline]
    pub(crate) fn get_mut(&mut self) -> &mut T {
        &mut self.storage
    }
}

/// A mutable reference to an entity's component with deferred change tracking.
///
/// A modification invent is only generated *if* if this is mutably dereferenced.
pub struct RefMut<'a, T> {
    guard: CellMutGuard<'a, T>,
    slot: Slot,
    modified: bool,
}

impl<'a, T: Debug> Debug for RefMut<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.guard.fmt(f)
    }
}

impl<'a, T> RefMut<'a, T> {
    pub(crate) fn new(guard: CellMutGuard<'a, [T]>, slot: Slot) -> Option<Self> {
        Some(RefMut {
            guard: guard.filter_map(|v| v.get_mut(slot))?,
            slot,
            modified: false,
        })
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
            self.guard.set_modified(Slice::single(self.slot));
        }
    }
}

// pub(crate) struct UniqueRefMut<'a, T> {
//     value: &'a mut T,
//     slot: Slot,
//     pub(super) cell: &'a mut Cell,
//     pub(super) ids: &'a [Entity],
//     pub(super) tick: u32,
// }

// impl<'a> Drop for UniqueRefMut<'a> {
//     #[inline]
//     fn drop(&mut self) {
//         self.cell
//             .on_event(self.ids, Slice::single(self.slot), EventKind::Modified);

//         self.cell
//             .changes
//             .get_mut()
//             .set_modified_if_tracking(Change::new(Slice::single(self.slot), self.tick));
//     }
// }
