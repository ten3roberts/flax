use crate::{
    archetype::{Archetype, Slice},
    fetch::{FetchAccessData, FetchPrepareData, PreparedFetch},
    system::Access,
    Fetch, FetchItem,
};
use alloc::vec::Vec;
use core::fmt::{self, Formatter};

#[derive(Debug, Clone)]
/// A filter that yields, well, nothing
pub struct Nothing;

impl FetchItem for Nothing {
    type Item<'q> = ();
}

impl<'a> Fetch<'a> for Nothing {
    const MUTABLE: bool = false;

    type Prepared = Nothing;

    #[inline(always)]
    fn prepare(&self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(Nothing)
    }

    #[inline(always)]
    fn filter_arch(&self, _: &Archetype) -> bool {
        false
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "false")
    }

    fn access(&self, _data: FetchAccessData, _dst: &mut Vec<Access>) {}
}

impl PreparedFetch for Nothing {
    type Item<'q> = ();

    unsafe fn fetch<'q>(&mut self, _: usize) -> Self::Item<'q> {}

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        Slice::new(slots.end, slots.end)
    }

    fn set_visited(&mut self, _slots: Slice) {}
}

/// Yields all entities
#[derive(Debug, Clone)]
pub struct All;

impl FetchItem for All {
    type Item<'q> = ();
}

impl<'w> Fetch<'w> for All {
    const MUTABLE: bool = false;

    type Prepared = All;

    fn prepare(&'w self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(All)
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "true")
    }

    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl PreparedFetch for All {
    type Item<'q> = ();

    unsafe fn fetch<'q>(&mut self, _: usize) -> Self::Item<'q> {}
}

impl FetchItem for Slice {
    type Item<'q> = ();
}

impl<'w> Fetch<'w> for Slice {
    const MUTABLE: bool = false;

    type Prepared = Self;

    fn prepare(&'w self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(*self)
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "slice {:?}", self)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl PreparedFetch for Slice {
    type Item<'q> = ();

    #[inline]
    unsafe fn fetch<'q>(&mut self, _: usize) -> Self::Item<'q> {}

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        self.intersect(&slots)
            .unwrap_or(Slice::new(slots.end, slots.end))
    }
}

impl FetchItem for bool {
    type Item<'q> = bool;
}

impl<'w> Fetch<'w> for bool {
    const MUTABLE: bool = false;

    type Prepared = Self;

    #[inline(always)]
    fn prepare(&'w self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(*self)
    }

    #[inline(always)]
    fn filter_arch(&self, _: &Archetype) -> bool {
        *self
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl PreparedFetch for bool {
    type Item<'q> = bool;

    #[inline]
    unsafe fn fetch<'q>(&mut self, _: usize) -> Self::Item<'q> {
        *self
    }
}
