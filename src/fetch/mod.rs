mod component;
mod ext;
mod opt;

use core::fmt::Debug;
use core::fmt::{self, Formatter};

use alloc::vec;
use alloc::vec::Vec;

pub use component::*;
pub use ext::*;
pub use opt::*;

use crate::filter::TupleOr;
use crate::{
    archetype::{Archetype, Slice, Slot},
    filter::Nothing,
    system::Access,
    ArchetypeId, ComponentKey, Entity, Filter, World,
};

#[doc(hidden)]
pub struct FmtQuery<'r, Q>(pub &'r Q);

impl<'r, 'w, Q> Debug for FmtQuery<'r, Q>
where
    Q: Fetch<'w>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.describe(f)
    }
}

/// Represents the world data necessary for preparing a fetch
#[derive(Copy, Clone)]
pub struct FetchPrepareData<'w> {
    /// The current world
    pub world: &'w World,
    /// The iterated archetype to prepare for
    pub arch: &'w Archetype,
    /// The archetype id
    pub arch_id: ArchetypeId,
}

/// Trait which gives an associated `Item` fetch type
pub trait FetchItem<'q> {
    /// The item yielded by the prepared fetch
    type Item;
}

/// Describes a type which can fetch itself from an archetype
pub trait Fetch<'w>: for<'q> FetchItem<'q> {
    /// true if the fetch mutates any component and thus needs a change event
    const MUTABLE: bool;
    /// true if the fetch has a filter
    const HAS_FILTER: bool = false;
    /// The filter associated to the fetch, if applicable. If the fetch does not
    /// use a filter, use the [ `crate::All` ] filter
    type Filter: for<'x> Filter<'x>;

    /// The prepared version of the fetch
    type Prepared: for<'x> PreparedFetch<'x, Item = <Self as FetchItem<'x>>::Item> + 'w;
    /// Prepare the query against an archetype. Returns None if doesn't match.
    /// If Self::matches true, this needs to return Some
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared>;

    /// Returns true if the fetch matches the archetype
    fn matches(&self, data: FetchPrepareData) -> bool;

    /// Returns which components and how will be accessed for an archetype.
    fn access(&self, data: FetchPrepareData) -> Vec<Access>;

    /// Describes the fetch in a human-readable fashion
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;

    /// Returns the filter if applicable
    fn filter(&self) -> Self::Filter;

    /// Returns the required component for the fetch.
    ///
    /// This is used for the query to determine which archetypes to visit
    fn components(&self, result: &mut Vec<ComponentKey>);
}

impl<'w> Fetch<'w> for () {
    const MUTABLE: bool = false;
    const HAS_FILTER: bool = false;
    type Filter = Nothing;

    type Prepared = ();

    fn prepare(&self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(())
    }

    fn matches(&self, _: FetchPrepareData) -> bool {
        true
    }

    fn access(&self, _: FetchPrepareData) -> Vec<Access> {
        vec![]
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "()")
    }

    fn filter(&self) -> Self::Filter {
        Nothing
    }

    fn components(&self, _: &mut Vec<ComponentKey>) {}
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
pub struct EntityIds;
#[doc(hidden)]
pub struct PreparedEntities<'a> {
    entities: &'a [Entity],
}

impl<'q> FetchItem<'q> for EntityIds {
    type Item = Entity;
}

impl<'w> Fetch<'w> for EntityIds {
    const MUTABLE: bool = false;
    const HAS_FILTER: bool = false;

    type Filter = Nothing;

    type Prepared = PreparedEntities<'w>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedEntities {
            entities: data.arch.entities(),
        })
    }

    fn matches(&self, _: FetchPrepareData) -> bool {
        true
    }

    fn access(&self, _: FetchPrepareData) -> Vec<Access> {
        vec![]
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("entity_ids")
    }

    fn filter(&self) -> Self::Filter {
        Nothing
    }

    fn components(&self, _: &mut Vec<ComponentKey>) {}
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
            type Filter         = TupleOr<($($ty::Filter,)*)>;
            const HAS_FILTER: bool =  $($ty::HAS_FILTER )|*;

            #[inline(always)]
            fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
                Some(($(
                    (self.$idx).prepare(data)?,
                )*))
            }

            #[inline(always)]
            fn matches(&self, data: FetchPrepareData) -> bool {
                ( $((self.$idx).matches(data)) && * )
            }

            #[inline(always)]
            fn describe(&self, f: &mut Formatter) -> fmt::Result {
                Debug::fmt(&($(FmtQuery(&self.$idx),)*), f)
            }

            #[inline(always)]
            fn access(&self, data: FetchPrepareData) -> Vec<Access> {
                [
                    $(
                        (self.$idx).access(data),
                    )*
                ].concat()
            }

            #[inline(always)]
            fn components(&self, result: &mut Vec<ComponentKey>) {
                $((self.$idx).components(result));*
            }

            #[inline(always)]
            fn filter(&self) -> Self::Filter {
                TupleOr(( $(self.$idx.filter(),)* ))
            }
        }

        impl<'q, $($ty, )*> PreparedFetch<'q> for ($($ty,)*)
        where $($ty: PreparedFetch<'q>,)*
        {
            type Item = ($(<$ty as PreparedFetch<'q>>::Item,)*);

            #[inline(always)]
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
