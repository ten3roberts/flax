mod component;
mod ext;
mod opt;
mod to_fetch;

use core::fmt;
use std::fmt::Write;

pub use component::*;
pub use ext::*;
pub use opt::*;

use crate::{
    archetype::{Archetype, Slice, Slot},
    system::Access,
    ArchetypeId, Entity, World,
};

/// Trait which gives an associated `Item` fetch type
pub trait FetchItem<'q> {
    /// The item yielded by the prepared fetch
    type Item;
}

/// Describes a type which can fetch itself from an archetype
pub trait Fetch<'w>: for<'q> FetchItem<'q> {
    /// true if the fetch mutates any component and thus needs a change event
    const MUTABLE: bool;

    /// The prepared version of the fetch
    type Prepared: for<'x> PreparedFetch<'x, Item = <Self as FetchItem<'x>>::Item> + 'w;
    /// Prepare the query against an archetype. Returns None if doesn't match.
    /// If Self::matches true, this needs to return Some
    fn prepare(&'w self, world: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared>;
    /// Returns true if the fetch matches the archetype
    fn matches(&self, world: &World, archetype: &Archetype) -> bool;
    /// Describes the fetch in a human-readable fashion
    fn describe(&self, f: &mut dyn Write) -> fmt::Result;
    /// Returns which components and how will be accessed for an archetype.
    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access>;
    /// Returns the required elements in self which are not in archetype
    fn difference(&self, archetype: &Archetype) -> Vec<String>;
}

impl<'w> Fetch<'w> for () {
    const MUTABLE: bool = false;

    type Prepared = ();

    fn prepare(&'w self, _: &'w World, _: &'w Archetype) -> Option<Self::Prepared> {
        Some(())
    }

    fn matches(&self, _: &World, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut dyn Write) -> fmt::Result {
        write!(f, "()")
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        vec![]
    }

    fn difference(&self, _: &Archetype) -> Vec<String> {
        vec![]
    }
}

impl<'q> FetchItem<'q> for () {
    type Item = ();
}

impl<'q> PreparedFetch<'q> for () {
    type Item = ();

    unsafe fn fetch(&'q mut self, _: Slot) -> Self::Item {}
}

/// A preborrowed fetch
pub trait PreparedFetch<'q>
where
    Self: Sized,
{
    /// The items yielded by the fetch
    type Item: Sized;
    /// Fetch the item from entity at the slot in the prepared storage.
    /// # Safety
    /// Must return non-aliased references to the underlying borrow of the
    /// prepared archetype.
    ///
    /// The callee is responsible for assuring disjoint calls.
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item;

    /// Do something for a a slice of entity slots which have been visited, such
    /// as updating change tracking for mutable queries. The current change tick
    /// is passed.
    ///
    /// # Safety
    /// The function can not modify any data which is returned by fetch.
    /// References to `Self::Item` are still alive when this function is called.
    /// As such, only disjoint data such as a separate Change borrow can be
    /// accessed
    unsafe fn set_visited(&mut self, _slots: Slice, _change_tick: u32) {}
}

#[derive(Debug, Clone)]
/// Returns the entity ids
pub struct Entities;
#[doc(hidden)]
pub struct PreparedEntities<'a> {
    entities: &'a [Entity],
}

impl<'q> FetchItem<'q> for Entities {
    type Item = Entity;
}

impl<'w> Fetch<'w> for Entities {
    const MUTABLE: bool = false;

    type Prepared = PreparedEntities<'w>;

    fn prepare(&'w self, _: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared> {
        Some(PreparedEntities {
            entities: archetype.entities(),
        })
    }

    fn matches(&self, _: &World, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut dyn Write) -> fmt::Result {
        f.write_str("entities")
    }

    fn difference(&self, _: &Archetype) -> Vec<String> {
        vec![]
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        vec![]
    }
}

impl<'w, 'q> PreparedFetch<'q> for PreparedEntities<'w> {
    type Item = Entity;

    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        self.entities[slot]
    }
}

// Implement for tuples
macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'q, $($ty, )*> FetchItem<'q> for ($($ty,)*)
            where $($ty: FetchItem<'q>,)*
        {
            type Item = ($($ty::Item,)*);

        }
        impl<'w, $($ty, )*> Fetch<'w> for ($($ty,)*)
            where $($ty: Fetch<'w>,)*
        {
            const MUTABLE: bool =  $($ty::MUTABLE )|*;
            type Prepared       = ($($ty::Prepared,)*);

            fn prepare(&'w self, world: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared> {
                Some(($(
                    (self.$idx).prepare(world, archetype)?,
                )*))
            }

            fn matches(&self, world: &World, archetype: &Archetype) -> bool {
                $((self.$idx).matches(world, archetype)) && *
            }

            fn describe(&self, f: &mut dyn Write) -> fmt::Result {
                f.write_str("(")?;
                $( (self.$idx).describe(f)?;)*
                f.write_str(")")
            }

            fn difference(&self, archetype: &Archetype) -> Vec<String> {
                [$((self.$idx).difference(archetype)),*].concat()
            }

            fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
                [ $(
                    (self.$idx).access(id, archetype),
                )* ].into_iter().flatten().collect()
            }

        }

        impl<'q, $($ty, )*> PreparedFetch<'q> for ($($ty,)*)
            where $($ty: PreparedFetch<'q>,)*
        {
            type Item = ($(<$ty as PreparedFetch<'q>>::Item,)*);

            unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
                ($(
                    (self.$idx).fetch(slot),
                )*)
            }

            unsafe fn set_visited(&mut self, slots: Slice, change_tick: u32) {
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
