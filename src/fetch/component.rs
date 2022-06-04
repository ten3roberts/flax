use core::slice;

use atomic_refcell::AtomicRefMut;

use crate::{
    archetype::{Archetype, Change, Changes, Slice, Slot, StorageBorrow, StorageBorrowMut},
    wildcard, Component, ComponentValue,
};

use super::*;

pub struct PreparedComponentMut<'a, T> {
    borrow: StorageBorrowMut<'a, T>,
    changes: AtomicRefMut<'a, Changes>,
}

pub struct PreparedComponent<'a, T> {
    borrow: StorageBorrow<'a, T>,
}

unsafe impl<'a, 'w, T: 'a> PreparedFetch for &'a PreparedComponent<'w, T> {
    type Item = &'a T;

    unsafe fn fetch(self, slot: Slot) -> Self::Item {
        self.borrow.at(slot)
    }
}

impl<'w, T> Fetch<'w> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedComponent<'w, T>;

    fn prepare(&self, _: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.storage(*self)?;
        Some(PreparedComponent { borrow })
    }

    fn matches(&self, _: &'w World, archetype: &'w Archetype) -> bool {
        archetype.has(self.id())
    }
}

pub struct Mutable<T>(pub(crate) Component<T>);

impl<'a, 'b, T> Fetch<'a> for Mutable<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = true;

    type Prepared = PreparedComponentMut<'a, T>;

    fn prepare(&self, _: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.storage_mut(self.0)?;
        let changes = archetype.changes_mut(self.0.id())?;

        Some(PreparedComponentMut { borrow, changes })
    }

    fn matches(&self, _: &'a World, archetype: &'a Archetype) -> bool {
        archetype.has(self.0.id())
    }
}

unsafe impl<'a, 'w, T: 'a> PreparedFetch for &'a PreparedComponentMut<'w, T> {
    type Item = &'a mut T;

    unsafe fn fetch(self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        &mut *(self.borrow.at(slot) as *const T as *mut T)
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        eprintln!("Setting changes for {slots:?}: {change_tick}");
        // TODO
        // self.changes.set(Change::modified(slots, change_tick));
    }
}

/// Similar to a component fetch, with the difference that it also yields the
/// object entity.
pub struct Relation<T> {
    component: Component<T>,
    index: usize,
}

impl<T> Relation<T> {
    pub fn new(component: Component<T>, index: usize) -> Self {
        Self { component, index }
    }
}

impl<'a, T> Fetch<'a> for Relation<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedPair<'a, T>;

    fn prepare(&self, world: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let (sub, obj) = self.component.id().into_pair();
        if obj == wildcard().id().strip_gen() {
            let (obj, borrow) = archetype
                .components()
                .filter(|v| v.id().strip_gen() == sub)
                .skip(self.index)
                .map(|v| {
                    let (sub1, obj) = v.id().into_pair();
                    assert_eq!(sub1, sub);
                    let borrow = archetype.storage_dyn::<T>(v.id()).unwrap();
                    let obj = world.reconstruct(obj).unwrap();
                    (obj, borrow)
                })
                .next()?;

            Some(PreparedPair { borrow, obj })
        } else {
            todo!()
        }
    }

    fn matches(&self, _: &'a World, archetype: &'a Archetype) -> bool {
        let (sub, obj) = self.component.id().into_pair();
        if obj == wildcard().id().strip_gen() {
            archetype
                .components()
                .filter(|component| component.id().strip_gen() == sub)
                .skip(self.index)
                .next()
                .is_some()
        } else {
            archetype.has(self.component.id())
        }
    }
}

pub struct PreparedPair<'a, T> {
    borrow: StorageBorrow<'a, T>,
    obj: Entity,
}

unsafe impl<'a, 'w, T> PreparedFetch for &'a PreparedPair<'w, T>
where
    T: ComponentValue,
{
    type Item = (Entity, &'a T);

    unsafe fn fetch(self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        let item = self.borrow.at(slot);
        (self.obj, item)
    }
}

pub struct PairMatchIter<'a, T> {
    borrow: slice::Iter<'a, (Entity, StorageBorrow<'a, T>)>,
    slot: Slot,
}

impl<'a, T> Iterator for PairMatchIter<'a, T> {
    type Item = (Entity, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let (id, borrow) = self.borrow.next()?;
        let item = unsafe { &*(borrow.at(self.slot) as *const T) };
        Some((*id, item))
    }
}
