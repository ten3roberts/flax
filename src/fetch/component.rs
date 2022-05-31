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

unsafe impl<'a, 'b, T> PreparedFetch<'a> for &'b mut PreparedComponent<'a, T> {
    type Item = &'a T;

    unsafe fn fetch(self, slot: Slot) -> Self::Item {
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

    fn prepare(&self, world: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.storage(*self)?;
        Some(PreparedComponent { borrow })
    }

    fn matches(&self, _: &'a World, archetype: &'a Archetype) -> bool {
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

    fn prepare(&self, _: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.storage_mut(self.0)?;
        let changes = archetype.changes_mut(self.0.id())?;

        Some(PreparedComponentMut { borrow, changes })
    }

    fn matches(&self, _: &'a World, archetype: &'a Archetype) -> bool {
        archetype.has(self.0.id())
    }
}

unsafe impl<'a, 'b, T: 'a> PreparedFetch<'a> for &'b mut PreparedComponentMut<'a, T> {
    type Item = &'a mut T;

    unsafe fn fetch(self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        &mut *(self.borrow.at(slot) as *const T as *mut T)
    }

    fn set_visited(self, slots: Slice, change_tick: u32) {
        eprintln!("Setting changes for {slots:?}: {change_tick}");
        self.changes.set(Change::modified(slots, change_tick));
    }
}

/* /// Similar to a component fetch, with the difference that it also yields the
/// object entity.
pub struct Pair<T>(pub(crate) Component<T>);

impl<'a, 'p, T> Fetch<'a> for &'p Pair<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Item = PairMatchIter<'a, T>;

    type Prepared = PreparedPair<'a, T>;

    fn prepare(&self, world: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let (sub, obj) = self.0.id().into_pair();
        if obj == wildcard().id().strip_gen() {
            let borrow = archetype
                .components()
                .filter(|v| v.id().strip_gen() == sub)
                .map(|v| {
                    let (sub1, obj) = v.id().into_pair();
                    assert_eq!(sub1, sub);
                    let borrow = archetype.storage_dyn::<T>(v.id()).unwrap();
                    let obj = world.reconstruct(obj).unwrap();
                    (obj, borrow)
                })
                .collect();

            Some(PreparedPair { borrow })
        } else {
            todo!()
        }
    }

    fn matches(&self, world: &'a World, archetype: &'a Archetype) -> bool {
        let (sub, obj) = self.0.id().into_pair();
        if obj == wildcard().id().strip_gen() {
            archetype
                .components()
                .find(|component| component.id().strip_gen() == sub);

            false
        } else {
            archetype.has(self.0.id())
        }
    }
}

pub struct PreparedPair<'a, T> {
    borrow: SmallVec<[(Entity, StorageBorrow<'a, T>); 4]>,
}

unsafe impl<'a, T> PreparedFetch<'a> for PreparedPair<'a, T>
where
    T: ComponentValue,
{
    type Item = PairMatchIter<'a, T>;

    unsafe fn fetch(&'a mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        PairMatchIter {
            borrow: self.borrow.iter(),
            slot,
        }
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
} */
