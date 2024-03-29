use super::{FetchAccessData, FmtQuery, PreparedFetch, RandomFetch};
use crate::{query::ArchetypeSearcher, system::Access, Fetch, FetchItem};
use alloc::vec::Vec;
use core::{fmt, ops::Deref};

/// Dereferences the fetch item
pub struct AsDeref<F>(pub F);

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
    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.0.filter_arch(data)
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
    type Chunk = F::Chunk;

    const HAS_FILTER: bool = F::HAS_FILTER;

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.0.filter_slots(slots)
    }

    unsafe fn create_chunk(&'q mut self, slots: crate::archetype::Slice) -> Self::Chunk {
        self.0.create_chunk(slots)
    }

    #[inline]
    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        F::fetch_next(chunk)
    }
}

impl<'q, F, V> RandomFetch<'q> for AsDeref<F>
where
    F: RandomFetch<'q, Item = &'q V>,
    V: 'static + Deref,
{
    unsafe fn fetch_shared(&'q self, slot: crate::archetype::Slot) -> Self::Item {
        self.0.fetch_shared(slot)
    }

    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: crate::archetype::Slot) -> Self::Item {
        F::fetch_shared_chunk(chunk, slot)
    }
}
