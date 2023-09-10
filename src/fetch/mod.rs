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
    filter::{RefFetch, StaticFilter},
    system::Access,
    util::Ptr,
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
pub use map::Map;
pub use maybe_mut::{MaybeMut, MutGuard};
pub use opt::*;
pub use read_only::*;
pub use relations::{relations_like, Relations, RelationsIter};
pub use satisfied::Satisfied;
pub use source::Source;
pub use transform::{Added, Modified, TransformFetch};

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

impl<'w> From<FetchPrepareData<'w>> for FetchAccessData<'w> {
    fn from(value: FetchPrepareData<'w>) -> Self {
        Self {
            world: value.world,
            arch: value.arch,
            arch_id: value.arch_id,
        }
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
    /// The tick the previous time the query executed
    pub old_tick: u32,
    /// The new tick to write if query is mutable
    pub new_tick: u32,
}

/// Trait which gives an associated `Item` fetch type
pub trait FetchItem<'q> {
    /// The item yielded by the prepared fetch
    type Item;
}

/// A fetch describes a retrieval of data from the world and archetypes during a query.
///
/// A fetch is prepared, wherein borrows are acquired and a `PreparedFetch` is returned, which is
/// used to provide the query with values.
///
/// The PreparedFetch can in turn control the ranges of slots which are requested by the query,
/// e.g; filtering changed components
pub trait Fetch<'w>: for<'q> FetchItem<'q> {
    /// true if the fetch mutates any component and thus needs a change event
    const MUTABLE: bool;

    /// The prepared version of the fetch
    type Prepared: for<'x> PreparedFetch<'x, Item = <Self as FetchItem<'x>>::Item> + 'w;

    /// Prepares the fetch for an archetype by acquiring borrows.
    ///
    /// Returns `None` if the archetype does not match.
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared>;

    /// Returns true if the archetype matches the fetch
    fn filter_arch(&self, data: FetchAccessData) -> bool;

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
pub trait PreparedFetch<'q> {
    /// Item returned by fetch
    type Item: 'q;
    /// A chunk accessing a disjoint set of the borrow sequentially
    type Chunk: 'q;

    /// Creates a chunk to access a slice of the borrow
    ///
    /// # Safety
    ///
    /// `slots` must be disjoint to all other currently existing chunks
    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk;

    /// Fetch the item from entity at the slot in the prepared storage.
    /// # Safety
    /// Must return non-aliased references to the underlying borrow of the
    /// prepared archetype.
    ///
    /// The callee is responsible for assuring disjoint calls.
    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item;

    #[inline]
    /// Filter the slots to visit
    /// Returns the leftmost subslice of `slots` which should be visited
    ///
    /// # Safety
    /// `slots` must not overlap any alive references returned by `fetch`
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        slots
    }
}

/// Allows filtering the constituent parts of a fetch using a set union
pub trait UnionFilter {
    // Filter the slots using a union operation of the constituent part
    ///
    /// # Safety
    /// See: [`PreparedFetch::filter_slots`]
    unsafe fn filter_union(&mut self, slots: Slice) -> Slice;
}

impl<'q, F> PreparedFetch<'q> for &'q mut F
where
    F: PreparedFetch<'q>,
{
    type Item = F::Item;
    type Chunk = F::Chunk;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        (*self).create_chunk(slots)
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        F::fetch_next(chunk)
    }

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        (*self).filter_slots(slots)
    }
}

impl<'q> FetchItem<'q> for () {
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

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "()")
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl<'q> RandomFetch<'q> for () {
    #[inline]
    unsafe fn fetch_shared(&'q self, _: Slot) -> Self::Item {}
    #[inline]
    unsafe fn fetch_shared_chunk(_: &Self::Chunk, _: Slot) -> Self::Item {}
}

impl<'q> PreparedFetch<'q> for () {
    type Item = ();

    type Chunk = ();

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {}

    #[inline]
    unsafe fn fetch_next(_: &mut Self::Chunk) -> Self::Item {}
}

// impl<'q, F> PreparedFetch<'q> for Option<F>
// where
//     F: PreparedFetch<'q>,
// {
//     type Item = Option<F::Item>;
//     type Chunk = Option<F::Chunk>;

//     unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
//         self.as_mut().map(|fetch| fetch.create_chunk(slots))
//     }

//     #[inline]
//     unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
//         if let Some(fetch) = self {
//             fetch.filter_slots(slots)
//         } else {
//             Slice::new(slots.end, slots.end)
//         }
//     }

//     unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
//         batch.as_mut().map(|fetch| F::fetch_next(fetch))
//     }
// }

#[derive(Debug, Clone)]
/// Returns the entity ids
pub struct EntityIds;
#[doc(hidden)]
pub struct ReadEntities<'a> {
    entities: &'a [Entity],
}

impl<'q> FetchItem<'q> for EntityIds {
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

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("entity_ids")
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl<'w, 'q> PreparedFetch<'q> for ReadEntities<'w> {
    type Item = Entity;

    type Chunk = Ptr<'q, Entity>;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        Ptr::new(self.entities[slots.as_range()].as_ptr())
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let old = chunk.as_ptr();
        *chunk = chunk.add(1);
        *old
    }
}

impl<'w, 'q> RandomFetch<'q> for ReadEntities<'w> {
    #[inline]
    unsafe fn fetch_shared(&self, slot: usize) -> Self::Item {
        self.entities[slot]
    }

    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
        *chunk.add(slot).as_ref()
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

        impl<'q, $($ty, )*> RandomFetch<'q> for ($($ty,)*)
        where $($ty: RandomFetch<'q>,)*
        {

            #[inline(always)]
            unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
                ($(
                    (self.$idx).fetch_shared(slot),
                )*)
            }

            #[inline(always)]
            unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
                ($(
                    $ty::fetch_shared_chunk(&chunk.$idx, slot),
                )*)
            }
        }


        impl<'q, $($ty, )*> PreparedFetch<'q> for ($($ty,)*)
            where $($ty: PreparedFetch<'q>,)*
        {

            type Item = ($($ty::Item,)*);
            type Chunk = ($($ty::Chunk,)*);

            #[inline]
            unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
                ($(
                    $ty::fetch_next(&mut chunk.$idx),
                )*)
            }

            #[inline]
            unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
                ($((self.$idx).create_chunk(slots),)*)
            }

            #[inline]
            unsafe fn filter_slots(&mut self, mut slots: Slice) -> Slice {
                $(

                    slots = self.$idx.filter_slots(slots);
                )*

                slots
            }
        }

        impl<'q, $($ty, )*> UnionFilter for ($($ty,)*)
            where $($ty: PreparedFetch<'q>,)*
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
            fn filter_arch(&self, data:FetchAccessData) -> bool {
                ( $((self.$idx).filter_arch(data)) && * )
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

        impl< $($ty: StaticFilter, )*> StaticFilter for ($($ty,)*)
        {
            fn filter_static(&self, arch: &Archetype) -> bool {
                ( $((self.$idx).filter_static(arch)) && * )
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
