use crate::{
    archetype::{Slice, Slot},
    fetch::{FetchAccessData, FetchPrepareData, PreparedFetch},
    system::Access,
    Entity, Fetch, FetchItem,
};
use alloc::vec::Vec;
use core::fmt::{self, Formatter};

use super::StaticFilter;

#[derive(Debug, Clone)]
/// A filter that yields, well, nothing
pub struct Nothing;

impl<'q> FetchItem<'q> for Nothing {
    type Item = ();
}

impl<'a> Fetch<'a> for Nothing {
    const MUTABLE: bool = false;

    type Prepared = Nothing;

    #[inline(always)]
    fn prepare(&self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(Nothing)
    }

    #[inline(always)]
    fn filter_arch(&self, _: FetchAccessData) -> bool {
        false
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "false")
    }

    fn access(&self, _data: FetchAccessData, _dst: &mut Vec<Access>) {}
}

impl StaticFilter for Nothing {
    fn filter_static(&self, _: &crate::archetype::Archetype) -> bool {
        false
    }
}

impl<'q> PreparedFetch<'q> for Nothing {
    type Item = ();
    type Chunk = ();

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        Slice::new(slots.end, slots.end)
    }

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {}

    #[inline]
    unsafe fn fetch_next(_: &mut Self::Chunk) -> Self::Item {}
}

/// Yields all entities
#[derive(Debug, Clone)]
pub struct All;

impl<'q> FetchItem<'q> for All {
    type Item = ();
}

impl<'w> Fetch<'w> for All {
    const MUTABLE: bool = false;

    type Prepared = All;

    fn prepare(&'w self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(All)
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "true")
    }

    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl StaticFilter for All {
    fn filter_static(&self, _: &crate::archetype::Archetype) -> bool {
        true
    }
}

impl<'q> PreparedFetch<'q> for All {
    type Item = ();

    type Chunk = ();

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {}

    #[inline]
    unsafe fn fetch_next(_: &mut Self::Chunk) -> Self::Item {}
}

impl<'w> FetchItem<'w> for Entity {
    type Item = Entity;
}

impl<'w> Fetch<'w> for Entity {
    const MUTABLE: bool = false;
    type Prepared = PreparedEntity;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let loc = data.world.location(*self).ok()?;

        if data.arch_id == loc.arch_id {
            Some(PreparedEntity {
                slot: loc.slot,
                id: *self,
            })
        } else {
            None
        }
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "entity {:?}", self)
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        if let Ok(loc) = data.world.location(*self) {
            data.arch_id == loc.arch_id
        } else {
            false
        }
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

#[doc(hidden)]
pub struct PreparedEntity {
    slot: Slot,
    id: Entity,
}

impl<'w> PreparedFetch<'w> for PreparedEntity {
    type Item = Entity;

    type Chunk = Entity;

    unsafe fn create_chunk(&'w mut self, slots: Slice) -> Self::Chunk {
        assert!(slots.start == self.slot && slots.end == self.slot);
        self.id
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        *chunk
    }

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if slots.contains(self.slot) {
            Slice::single(self.slot)
        } else {
            Slice::new(slots.end, slots.end)
        }
    }
}

impl<'q> FetchItem<'q> for Slice {
    type Item = ();
}

impl<'w> Fetch<'w> for Slice {
    const MUTABLE: bool = false;

    type Prepared = Self;

    fn prepare(&'w self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(*self)
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "slice {:?}", self)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl<'q> PreparedFetch<'q> for Slice {
    type Item = ();
    type Chunk = ();

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        self.intersect(&slots)
            .unwrap_or(Slice::new(slots.end, slots.end))
    }

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {}

    #[inline]
    unsafe fn fetch_next(_: &mut Self::Chunk) -> Self::Item {}
}

impl<'q> FetchItem<'q> for bool {
    type Item = bool;
}

impl<'w> Fetch<'w> for bool {
    const MUTABLE: bool = false;

    type Prepared = Self;

    #[inline(always)]
    fn prepare(&'w self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(*self)
    }

    #[inline(always)]
    fn filter_arch(&self, _: FetchAccessData) -> bool {
        *self
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl StaticFilter for bool {
    fn filter_static(&self, _: &crate::archetype::Archetype) -> bool {
        *self
    }
}

impl<'q> PreparedFetch<'q> for bool {
    type Item = bool;

    type Chunk = bool;

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {
        *self
    }

    #[inline]
    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        *chunk
    }
}
