use crate::{
    archetype::{Archetype, Slot, StorageBorrow},
    Component, ComponentValue,
};

/// Describes a type which can fetch itself from an archetype
pub trait Fetch<'a> {
    type Item;
    type Prepared: PreparedFetch<'a, Item = Self::Item>;
    fn prepare(&self, archetype: &'a Archetype) -> Self::Prepared;
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
}

pub struct PreparedComponent<'a, T> {
    borrow: StorageBorrow<'a, T>,
}

unsafe impl<'a, T> PreparedFetch<'a> for PreparedComponent<'a, T> {
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
    type Item = &'a T;

    type Prepared = PreparedComponent<'a, T>;

    fn prepare(&self, archetype: &'a Archetype) -> Self::Prepared {
        PreparedComponent {
            borrow: archetype.storage(*self).unwrap(),
        }
    }

    fn matches(&self, archetype: &'a Archetype) -> bool {
        archetype.has(self.id())
    }
}

// Implement for tuples
macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'a, $($ty, )*> Fetch<'a> for ($($ty,)*)
            where $($ty: Fetch<'a>,)*
        {
            type Item = ($(<$ty as Fetch<'a>>::Item,)*);
            type Prepared = ($(<$ty as Fetch<'a>>::Prepared,)*);

            fn prepare(&self, archetype: &'a Archetype) -> Self::Prepared {
                ($(
                    (self.$idx).prepare(archetype),
                )*)
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
