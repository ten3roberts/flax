use atomic_refcell::AtomicRefMut;

use crate::{
    archetype::{Archetype, Change, Changes, Slice, Slot, StorageBorrow, StorageBorrowMut},
    Component, ComponentValue,
};

use super::*;

pub struct PreparedComponentMut<'a, T> {
    borrow: StorageBorrowMut<'a, T>,
    changes: AtomicRefMut<'a, Changes>,
}

pub struct PreparedComponent<'a, T> {
    borrow: StorageBorrow<'a, T>,
}

unsafe impl<'a, T: 'a> PreparedFetch<'a> for PreparedComponent<'a, T> {
    type Item = &'a T;

    unsafe fn fetch(&mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        &*(self.borrow.at(slot) as *const T)
    }
}

impl<'a, T> Fetch<'a> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Item = &'a T;

    type Prepared = PreparedComponent<'a, T>;

    fn prepare(&self, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.storage(*self)?;
        Some(PreparedComponent { borrow })
    }

    fn matches(&self, archetype: &'a Archetype) -> bool {
        archetype.has(self.id())
    }
}

pub struct Mutable<T>(pub(crate) Component<T>);

impl<'a, T> Fetch<'a> for Mutable<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = true;

    type Item = &'a mut T;

    type Prepared = PreparedComponentMut<'a, T>;

    fn prepare(&self, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.storage_mut(self.0)?;
        let changes = archetype.changes_mut(self.0.id())?;

        Some(PreparedComponentMut { borrow, changes })
    }

    fn matches(&self, archetype: &'a Archetype) -> bool {
        archetype.has(self.0.id())
    }
}

unsafe impl<'a, T: 'a> PreparedFetch<'a> for PreparedComponentMut<'a, T> {
    type Item = &'a mut T;

    unsafe fn fetch(&mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        &mut *(self.borrow.at(slot) as *const T as *mut T)
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        eprintln!("Setting changes for {slots:?}: {change_tick}");
        self.changes.set(Change::modified(slots, change_tick));
    }
}
