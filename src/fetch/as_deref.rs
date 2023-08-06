use super::{FetchAccessData, FmtQuery, PreparedFetch, ReadOnlyFetch};
use crate::{query::ArchetypeSearcher, system::Access, Fetch, FetchItem};
use alloc::vec::Vec;
use core::{fmt, ops::Deref};

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
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.0.access(data, dst)
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

    type Batch = F::Batch;

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.0.filter_slots(slots)
    }

    unsafe fn create_chunk(&'q mut self, slots: crate::archetype::Slice) -> Self::Batch {
        self.0.create_chunk(slots)
    }

    unsafe fn fetch_next(batch: &mut Self::Batch) -> Self::Item {
        F::fetch_next(batch)
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
