mod as_deref;
mod cloned;
mod component;
mod component_mut;
mod copied;
mod entity_ref;
mod ext;
mod map;
mod maybe_mut;
mod opt;
mod read_only;
mod relations;
mod satisfied;
mod source;
mod transform;

use crate::{
    archetype::{Archetype, Slice, Slot},
    filter::RefFetch,
    system::Access,
    ArchetypeId, ArchetypeSearcher, Entity, World,
};
use alloc::vec::Vec;
use core::fmt::Debug;
use core::fmt::{self, Formatter};

pub use as_deref::*;
pub use cloned::*;
pub use component::*;
pub use component_mut::*;
pub use entity_ref::*;
pub use ext::FetchExt;
pub use maybe_mut::{MaybeMut, MutGuard};
pub use opt::*;
pub use read_only::*;
pub use relations::{relations_like, Relations, RelationsIter};
pub use satisfied::Satisfied;
pub use source::Source;
pub use transform::{Modified, TransformFetch};

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

/// Represents the world data necessary for declaring fetch access
#[derive(Copy, Clone)]
pub struct FetchAccessData<'w> {
    /// The current world
    pub world: &'w World,
    /// The iterated archetype to prepare for
    pub arch: &'w Archetype,
    /// The archetype id
    pub arch_id: ArchetypeId,
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
    /// The tick the previous time the query executed
    pub old_tick: u32,
    /// The new tick to write if query is mutable
    pub new_tick: u32,
}

/// Trait which gives an associated `Item` fetch type
pub trait FetchItem {
    /// The item yielded by the prepared fetch
    type Item<'q>;
}

/// A fetch describes a retrieval of data from the world and archetypes during a query.
///
/// A fetch is prepared, wherein borrows are acquired and a `PreparedFetch` is returned, which is
/// used to provide the query with values.
///
/// The PreparedFetch can in turn control the ranges of slots which are requested by the query,
/// e.g; filtering changed components
pub trait Fetch<'w>: FetchItem {
    /// true if the fetch mutates any component and thus needs a change event
    const MUTABLE: bool;

    /// The prepared version of the fetch
    type Prepared: for<'x> PreparedFetch<Item<'x> = <Self as FetchItem>::Item<'x>> + 'w;

    /// Prepares the fetch for an archetype by acquiring borrows.
    ///
    /// Returns `None` if the archetype does not match.
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared>;

    /// Rough filter to exclude or include archetypes.
    fn filter_arch(&self, arch: &Archetype) -> bool;

    /// Returns which components and how will be accessed for an archetype.
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>);

    /// Describes the fetch in a human-readable fashion
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;

    /// Returns the required component for the fetch.
    ///
    /// This is used for the query to determine which archetypes to visit
    #[inline]
    fn searcher(&self, _searcher: &mut ArchetypeSearcher) {}

    /// Convert the fetch to a reference type which works with `HRTB`
    #[inline]
    fn by_ref(&self) -> RefFetch<Self>
    where
        Self: Sized,
    {
        RefFetch(self)
    }
}

/// Borrowed state for a fetch
pub trait PreparedFetch {
    /// Item returned by fetch
    type Item<'q>: 'q
    where
        Self: 'q;

    /// Fetch the item from entity at the slot in the prepared storage.
    /// # Safety
    /// Must return non-aliased references to the underlying borrow of the
    /// prepared archetype.
    ///
    /// The callee is responsible for assuring disjoint calls.
    unsafe fn fetch<'q>(&'q mut self, slot: usize) -> Self::Item<'q>;

    #[inline]
    /// Filter the slots to visit
    /// Returns the leftmost subslice of `slots` which should be visited
    ///
    /// # Safety
    /// `slots` must not overlap any alive references returned by `fetch`
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        slots
    }

    /// Do something for a slice of entity slots which have been visited, such
    /// as updating change tracking for mutable queries.
    #[inline]
    fn set_visited(&mut self, _slots: Slice) {}
}

/// Allows filtering the constituent parts of a fetch using a set union
pub trait UnionFilter {
    // Filter the slots using a union operation of the constituent part
    ///
    /// # Safety
    /// See: [`PreparedFetch::filter_slots`]
    unsafe fn filter_union(&mut self, slots: Slice) -> Slice;
}

impl<F> PreparedFetch for &mut F
where
    F: PreparedFetch,
{
    type Item<'q> = F::Item<'q>where F: 'q;

    unsafe fn fetch<'q>(&'q mut self, slot: usize) -> Self::Item<'q> {
        (*self).fetch(slot)
    }

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        (*self).filter_slots(slots)
    }

    fn set_visited(&mut self, slots: Slice) {
        (*self).set_visited(slots)
    }
}

impl FetchItem for () {
    type Item = ();
}

impl UnionFilter for () {
    unsafe fn filter_union(&mut self, slots: Slice) -> Slice {
        slots
    }
}

impl<'w> Fetch<'w> for () {
    const MUTABLE: bool = false;

    type Prepared = ();

    fn prepare(&self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(())
    }

    fn filter_arch(&self, _arch: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "()")
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl ReadOnlyFetch for () {
    unsafe fn fetch_shared<'q>(&'q self, _: Slot) -> Self::Item<'q> {}
}

impl PreparedFetch for () {
    type Item<'q> = ();

    unsafe fn fetch<'q>(&'q mut self, _: Slot) -> Self::Item<'q> {}
}

impl<F> PreparedFetch for Option<F>
where
    F: PreparedFetch,
{
    type Item<'q> = Option<F::Item<'q>>where
        Self: 'q;

    #[inline]
    unsafe fn fetch<'q>(&'q mut self, slot: usize) -> Self::Item<'q> {
        self.as_mut().map(|fetch| fetch.fetch(slot))
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = self {
            fetch.filter_slots(slots)
        } else {
            Slice::new(slots.end, slots.end)
        }
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice) {
        if let Some(fetch) = self {
            fetch.set_visited(slots)
        }
    }
}

#[derive(Debug, Clone)]
/// Returns the entity ids
pub struct EntityIds;
#[doc(hidden)]
pub struct ReadEntities<'a> {
    entities: &'a [Entity],
}

impl FetchItem for EntityIds {
    type Item = Entity;
}

impl<'w> Fetch<'w> for EntityIds {
    const MUTABLE: bool = false;

    type Prepared = ReadEntities<'w>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(ReadEntities {
            entities: data.arch.entities(),
        })
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("entity_ids")
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl<'w> PreparedFetch for ReadEntities<'w> {
    type Item<'q> = Entity;

    #[inline]
    unsafe fn fetch<'q>(&'q mut self, slot: usize) -> Self::Item<'q> {
        self.entities[slot]
    }
}

impl<'w> ReadOnlyFetch for ReadEntities<'w> {
    #[inline]
    unsafe fn fetch_shared<'q>(&'q self, slot: usize) -> Self::Item<'q> {
        self.entities[slot]
    }
}

// Implement for tuples
macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<$($ty, )*> FetchItem for ($($ty,)*)
        where $($ty: FetchItem,)*
        {
            type Item<'q> = ($($ty::Item<'q>,)*);

        }

        impl<$($ty, )*> ReadOnlyFetch for ($($ty,)*)
        where $($ty: ReadOnlyFetch,)*
        {

            #[inline(always)]
            unsafe fn fetch_shared<'q>(&'q self, slot: Slot) -> Self::Item<'q> {
                ($(
                    (self.$idx).fetch_shared(slot),
                )*)
            }
        }


        impl<$($ty, )*> PreparedFetch for ($($ty,)*)
            where $($ty: PreparedFetch,)*
        {

            type Item<'q> = ($($ty::Item<'q>,)*) where Self: 'q;

            #[inline]
            unsafe fn fetch<'q>(&'q mut self, slot: Slot) -> Self::Item<'q> {
                ($(
                    (self.$idx).fetch(slot),
                )*)
            }

            #[inline]
            fn set_visited(&mut self, slots: Slice) {
                $((self.$idx).set_visited(slots);)*
            }

            #[inline]
            unsafe fn filter_slots(&mut self, mut slots: Slice) -> Slice {
                $(

                    slots = self.$idx.filter_slots(slots);
                )*

                slots
            }
        }

        impl<$($ty, )*> UnionFilter for ($($ty,)*)
            where $($ty: PreparedFetch,)*
        {

            #[inline]
            unsafe fn filter_union(&mut self, slots: Slice) -> Slice {
                [
                    // Don't leak union into this
                    $( self.$idx.filter_slots(slots)),*
                ]
                .into_iter()
                .min()
                .unwrap_or_default()
            }
        }

        impl<'w, $($ty, )*> Fetch<'w> for ($($ty,)*)
        where $($ty: Fetch<'w>,)*
        {
            const MUTABLE: bool =  $($ty::MUTABLE )|*;
            type Prepared       = ($($ty::Prepared,)*);

            #[inline]
            fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
                Some( ($( (self.$idx).prepare(data)?,)*) )
            }

            #[inline]
            fn filter_arch(&self, arch: &Archetype) -> bool {
                ( $((self.$idx).filter_arch(arch)) && * )
            }

            #[inline]
            fn describe(&self, f: &mut Formatter) -> fmt::Result {
                Debug::fmt(&($(FmtQuery(&self.$idx),)*), f)
            }

            #[inline]
            fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
                $( (self.$idx).access(data, dst);)*
            }

            #[inline]
            fn searcher(&self, searcher: &mut ArchetypeSearcher) {
                $((self.$idx).searcher(searcher));*
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
