mod ext;
mod filter;

use crate::{
    archetype::{Archetype, EntitySlice, Slot, StorageBorrow, StorageBorrowMut},
    Component, ComponentValue,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareInfo {
    /// The current change tick of the world
    pub old_tick: u32,
    pub new_tick: u32,
    pub slots: EntitySlice,
}

/// Describes a type which can fetch itself from an archetype
pub trait Fetch<'a> {
    const MUTABLE: bool;

    type Item;
    type Prepared: PreparedFetch<'a, Item = Self::Item>;
    fn prepare(&self, archetype: &'a Archetype, info: &PrepareInfo) -> Option<Self::Prepared>;
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
    unsafe fn fetch(&mut self, slot: Slot) -> Self::Item;
}

pub struct PreparedComponentMut<'a, T> {
    borrow: StorageBorrowMut<'a, T>,
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

    fn prepare(&self, archetype: &'a Archetype, info: &PrepareInfo) -> Option<Self::Prepared> {
        let borrow = archetype.storage(*self)?;
        Some(PreparedComponent { borrow })
    }

    fn matches(&self, archetype: &'a Archetype) -> bool {
        archetype.has(self.id())
    }
}

pub struct Mutable<T>(pub(crate) Component<T>);

unsafe impl<'a, T: 'a> PreparedFetch<'a> for PreparedComponentMut<'a, T> {
    type Item = &'a mut T;

    unsafe fn fetch(&mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        &mut *(self.borrow.at_mut(slot) as *mut T)
    }
}
impl<'a, T> Fetch<'a> for Mutable<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = true;

    type Item = &'a mut T;

    type Prepared = PreparedComponentMut<'a, T>;

    fn prepare(&self, archetype: &'a Archetype, info: &PrepareInfo) -> Option<Self::Prepared> {
        // Marked the prepared range as mutated
        archetype
            .changes_mut(self.0.id())?
            .set(info.slots, info.new_tick);

        let borrow = archetype.storage_mut(self.0)?;
        Some(PreparedComponentMut { borrow })
    }

    fn matches(&self, archetype: &'a Archetype) -> bool {
        archetype.has(self.0.id())
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

            fn prepare(&self, archetype: &'a Archetype, info: &PrepareInfo) -> Option<Self::Prepared> {
                Some(($(
                    (self.$idx).prepare(archetype, info)?,
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

            unsafe fn fetch(&mut self, slot: Slot) -> Self::Item {
                ($(
                    (self.$idx).fetch(slot),
                )*)
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
