mod ext;
mod filter;

pub use ext::*;
pub use filter::*;

use atomic_refcell::AtomicRefMut;

use crate::{
    archetype::{Archetype, Changes, Slice, Slot, StorageBorrow, StorageBorrowMut},
    Component, ComponentValue,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareInfo {
    /// The current change tick of the world
    pub old_tick: u32,
    pub new_tick: u32,
    pub slots: Slice,
}

/// Describes a type which can fetch itself from an archetype
pub trait Fetch<'a> {
    const MUTABLE: bool;

    type Item;
    type Prepared: PreparedFetch<'a, Item = Self::Item>;
    /// Prepare the query against an archetype. Returns None if doesn't match
    fn prepare(&self, archetype: &'a Archetype) -> Option<Self::Prepared>;
    fn matches(&self, archetype: &'a Archetype) -> bool;
}

/// A preborrowed fetch
pub unsafe trait PreparedFetch<'a> {
    type Item;
    /// Fetch the item from entity at the slot in the prepared storage.
    /// # Safety
    /// Must return non-aliased references to the underlying borrow of the
    /// prepared archetype.
    ///
    /// The callee is responsible for assuring disjoint calls.
    unsafe fn fetch(&self, slot: Slot) -> Self::Item;

    // Do something for a a slice of entity slots which have been visited, such
    // as updating change tracking for mutable queries. The current change tick
    // is passed.
    fn set_visited(&mut self, _slots: Slice, _change_tick: u32) {}
}

pub struct PreparedComponentMut<'a, T> {
    borrow: StorageBorrowMut<'a, T>,
    changes: AtomicRefMut<'a, Changes>,
}

pub struct PreparedComponent<'a, T> {
    borrow: StorageBorrow<'a, T>,
}

unsafe impl<'a, T: 'a> PreparedFetch<'a> for PreparedComponent<'a, T> {
    type Item = &'a T;

    unsafe fn fetch(&self, slot: Slot) -> Self::Item {
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

    unsafe fn fetch(&self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        &mut *(self.borrow.at(slot) as *const T as *mut T)
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        eprintln!("Setting changes for {slots:?}: {change_tick}");
        self.changes.set(slots, change_tick);
    }
}

// Implement for tuples
macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'a, $($ty, )*> Fetch<'a> for ($($ty,)*)
            where $($ty: Fetch<'a>,)*
        {
            const MUTABLE: bool = $(<$ty as Fetch<'a>>::MUTABLE )|*;
            type Item = ($(<$ty as Fetch<'a>>::Item,)*);
            type Prepared = ($(<$ty as Fetch<'a>>::Prepared,)*);

            fn prepare(&self, archetype: &'a Archetype) -> Option<Self::Prepared> {
                Some(($(
                    (self.$idx).prepare(archetype)?,
                )*))
            }

            fn matches(&self, archetype: &'a Archetype) -> bool {
                $((self.$idx).matches(archetype)) && *
            }
        }

        unsafe impl<'a, $($ty, )*> PreparedFetch<'a> for ($($ty,)*)
            where $($ty: PreparedFetch<'a>,)*
        {
            type Item = ($(<$ty as PreparedFetch<'a>>::Item,)*);

            unsafe fn fetch(&self, slot: Slot) -> Self::Item {
                ($(
                    (self.$idx).fetch(slot),
                )*)
            }

            fn set_visited(&mut self, slots: Slice, change_tick: u32) {
                $((self.$idx).set_visited(slots, change_tick);)*
            }
        }
    };
}

tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }
