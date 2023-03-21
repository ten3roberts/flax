use core::{fmt, ops::Deref};

use alloc::vec::Vec;

use crate::{query::ArchetypeSearcher, Fetch, FetchItem};

use super::{FmtQuery, PreparedFetch, ReadOnlyFetch};

/// Dereferences the fetch item
pub struct AsDeref<F>(pub(crate) F);

impl<'q, F, V> FetchItem<'q> for AsDeref<F>
where
    F: FetchItem<'q, Item = &'q V>,
    V: 'static + Deref,
{
    type Item = &'q V::Target;
}

impl<'w, F, V> Fetch<'w> for AsDeref<F>
where
    F: Fetch<'w>,
    F: for<'q> FetchItem<'q, Item = &'q V>,
    V: 'static + Deref,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = AsDeref<F::Prepared>;

    #[inline]
    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(AsDeref(self.0.prepare(data)?))
    }

    #[inline]
    fn filter_arch(&self, arch: &crate::archetype::Archetype) -> bool {
        self.0.filter_arch(arch)
    }

    #[inline]
    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "deref {:?}", FmtQuery(&self.0))
    }

    #[inline]
    fn access(&self, data: super::FetchAccessData) -> Vec<crate::Access> {
        self.0.access(data)
    }

    #[inline]
    fn searcher(&self, searcher: &mut ArchetypeSearcher) {
        self.0.searcher(searcher)
    }
}

impl<'q, F, V> PreparedFetch<'q> for AsDeref<F>
where
    F: PreparedFetch<'q, Item = &'q V>,
    V: 'static + Deref,
{
    type Item = &'q V::Target;

    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        self.0.fetch(slot)
    }

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.0.filter_slots(slots)
    }

    fn set_visited(&mut self, slots: crate::archetype::Slice) {
        self.0.set_visited(slots)
    }
}

impl<'q, F, V> ReadOnlyFetch<'q> for AsDeref<F>
where
    F: ReadOnlyFetch<'q, Item = &'q V>,
    V: 'static + Deref,
{
    unsafe fn fetch_shared(&'q self, slot: crate::archetype::Slot) -> Self::Item {
        self.0.fetch_shared(slot)
    }
}
