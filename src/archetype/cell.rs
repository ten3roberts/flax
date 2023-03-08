use core::ops::{Deref, DerefMut};

use alloc::sync::Arc;
use atomic_refcell::AtomicRefMut;

use crate::{
    events::{BufferedEvent, EventHandler, EventKind},
    Entity,
};

use super::{Cell, Change, Changes, Slice, Slot};

pub(crate) struct CellMutGuard<'a, T> {
    pub(crate) storage: AtomicRefMut<'a, [T]>,
    pub(super) changes: AtomicRefMut<'a, Changes>,
    pub(super) cell: &'a Cell,
    pub(super) ids: &'a [Entity],
    pub(super) tick: u32,
}

impl<'a, T> CellMutGuard<'a, T> {
    pub(crate) fn set_modified(&mut self, slots: Slice) {
        let event = BufferedEvent {
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

    pub(crate) fn storage(&self) -> &AtomicRefMut<'a, [T]> {
        &self.storage
    }

    pub(crate) fn storage_mut(&mut self) -> &mut AtomicRefMut<'a, [T]> {
        &mut self.storage
    }
}
