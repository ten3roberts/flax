mod cloned;
mod component;
mod copied;
mod entity_ref;
mod ext;
mod opt;

use core::fmt::Debug;
use core::fmt::{self, Formatter};

use alloc::vec;
use alloc::vec::Vec;

pub use cloned::*;
pub use component::*;
pub use entity_ref::*;
pub use ext::*;
pub use opt::*;

use crate::filter::RefFetch;
use crate::{
    archetype::{Archetype, Slice, Slot},
    system::Access,
    ArchetypeId, ArchetypeSearcher, Entity, World,
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

    /// Prepares the fetch for an archetype
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared>;

    /// Rough filter to exclude or include archetypes.
    fn filter_arch(&self, arch: &Archetype) -> bool;

    /// Returns which components and how will be accessed for an archetype.
    #[inline]
    fn access(&self, _: FetchPrepareData) -> Vec<Access> {
        Vec::new()
    }

    /// Describes the fetch in a human-readable fashion
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;

    /// Returns the required component for the fetch.
    ///
    /// This is used for the query to determine which archetypes to visit
    #[inline]
    fn searcher(&self, _: &mut ArchetypeSearcher) {}

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
    // Fetch the item from entity at the slot in the prepared storage.
    /// # Safety
    /// Must return non-aliased references to the underlying borrow of the
    /// prepared archetype.
    ///
    /// The callee is responsible for assuring disjoint calls.
    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item;

    /// Filter the slots to visit
    #[inline]
    fn filter_slots(&mut self, slots: Slice) -> Slice {
        slots
    }

    /// Do something for a a slice of entity slots which have been visited, such
    /// as updating change tracking for mutable queries. The current change tick
    /// is passed.
    #[inline]
    fn set_visited(&mut self, _slots: Slice, _change_tick: u32) {}
}

impl<'q, F> PreparedFetch<'q> for &'q mut F
where
    F: PreparedFetch<'q>,
{
    type Item = F::Item;

    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        (*self).fetch(slot)
    }

    fn filter_slots(&mut self, slots: Slice) -> Slice {
        (*self).filter_slots(slots)
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        (*self).set_visited(slots, change_tick)
    }
}

impl<'q> FetchItem<'q> for () {
    type Item = ();
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
}

impl<'q> PreparedFetch<'q> for () {
    type Item = ();

    unsafe fn fetch(&'q mut self, _: Slot) -> Self::Item {}
}

impl<'q, F> PreparedFetch<'q> for Option<F>
where
    F: PreparedFetch<'q>,
{
    type Item = Option<F::Item>;

    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        self.as_mut().map(|fetch| fetch.fetch(slot))
    }

    fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = self {
            fetch.filter_slots(slots)
        } else {
            Slice::empty()
        }
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        if let Some(fetch) = self {
            fetch.set_visited(slots, change_tick)
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

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("entity_ids")
    }
}

impl<'w, 'q> PreparedFetch<'q> for ReadEntities<'w> {
    type Item = Entity;

    #[inline]
    unsafe fn fetch(&mut self, slot: usize) -> Self::Item {
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

        impl<'q, $($ty, )*> PreparedFetch<'q> for ($($ty,)*)
            where $($ty: PreparedFetch<'q>,)*
        {

            type Item           = ($($ty::Item,)*);
            #[inline]
            unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
                ($(
                    (self.$idx).fetch(slot),
                )*)
            }

            #[inline]
            fn set_visited(&mut self, slots: Slice, change_tick: u32) {
                $((self.$idx).set_visited(slots, change_tick);)*
            }

            #[inline]
            fn filter_slots(&mut self, mut slots: Slice) -> Slice {
                let mut start = slots.start;

                ( $(
                    {
                        let v = self.$idx.filter_slots(slots);
                        eprintln!("{:?}: {:?} {:?}", std::any::type_name::<$ty>(), stringify!($idx), v);
                        start = start.max(v.start);
                        v
                    },
                )*);

                let mut u = slots;

                // Clamp to end bound
                start = start.min(slots.end);
                eprintln!("Found common start: {slots:?} => {start}");
                slots.start = start;

                $(
                    u = u.intersect(&self.$idx.filter_slots(slots));
                )*

                u
            }
        }

        impl<'w, $($ty, )*> Fetch<'w> for ($($ty,)*)
        where $($ty: Fetch<'w>,)*
        {
            const MUTABLE: bool =  $($ty::MUTABLE )|*;
            type Prepared       = ($($ty::Prepared,)*);

            #[inline]
            fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
                Some(($(
                    (self.$idx).prepare(data)?,
                )*))
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
            fn access(&self, data: FetchPrepareData) -> Vec<Access> {
                [
                    $(
                        (self.$idx).access(data),
                    )*
                ].concat()
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
