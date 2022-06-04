mod component;
mod ext;
mod filter;

pub use component::*;
pub use ext::*;
pub use filter::*;

use crate::{
    archetype::{Archetype, Slice, Slot},
    Entity, World,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareInfo {
    /// The current change tick of the world
    pub old_tick: u32,
    pub new_tick: u32,
    pub slots: Slice,
}

/// Describes a type which can fetch itself from an archetype
pub trait Fetch<'w> {
    const MUTABLE: bool;

    type Prepared;
    /// Prepare the query against an archetype. Returns None if doesn't match.
    /// If Self::matches true, this needs to return Some
    fn prepare(&self, world: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared>;
    fn matches(&self, world: &'w World, archetype: &'w Archetype) -> bool;
}

/// A preborrowed fetch
pub unsafe trait PreparedFetch
where
    Self: Sized,
{
    type Item;
    /// Fetch the item from entity at the slot in the prepared storage.
    /// # Safety
    /// Must return non-aliased references to the underlying borrow of the
    /// prepared archetype.
    ///
    /// The callee is responsible for assuring disjoint calls.
    unsafe fn fetch(self, slot: Slot) -> Self::Item;

    // Do something for a a slice of entity slots which have been visited, such
    // as updating change tracking for mutable queries. The current change tick
    // is passed.
    fn set_visited(&mut self, _slots: Slice, _change_tick: u32) {}
}

pub struct EntityFetch;
pub struct PreparedEntities<'a> {
    entities: &'a [Option<Entity>],
}

impl<'a> Fetch<'a> for EntityFetch {
    const MUTABLE: bool = false;

    type Prepared = PreparedEntities<'a>;

    fn prepare(&self, world: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
        Some(PreparedEntities {
            entities: archetype.entities(),
        })
    }

    fn matches(&self, world: &'a World, _: &'a Archetype) -> bool {
        true
    }
}

unsafe impl<'w> PreparedFetch for &PreparedEntities<'w> {
    type Item = Entity;

    unsafe fn fetch(self, slot: Slot) -> Self::Item {
        self.entities[slot].unwrap()
    }
}

// Implement for tuples
macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'a, 'b, $($ty, )*> Fetch<'a> for ($($ty,)*)
            where $($ty: Fetch<'a>,)*
        {
            const MUTABLE: bool =  $($ty::MUTABLE )|*;
            type Prepared       = ($($ty::Prepared,)*);

            fn prepare(&self, world: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
                Some(($(
                    (self.$idx).prepare(world, archetype)?,
                )*))
            }

            fn matches(&self, world: &'a World, archetype: &'a Archetype) -> bool {
                $((self.$idx).matches(world, archetype)) && *
            }
        }

        unsafe impl<'a, $($ty, )*> PreparedFetch for &'a ($($ty,)*)
            where $(&'a $ty: PreparedFetch,)*
        {
            type Item = ($(<&'a $ty as PreparedFetch>::Item,)*);

            unsafe fn fetch(&'a self, slot: Slot) -> Self::Item {
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

// tuple_impl! { 0 => A }
// tuple_impl! { 0 => A, 1 => B }
// tuple_impl! { 0 => A, 1 => B, 2 => C }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }
