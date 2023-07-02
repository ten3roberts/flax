use super::{FetchAccessData, FmtQuery, PreparedFetch, ReadOnlyFetch};
use crate::{query::ArchetypeSearcher, system::Access, Fetch, FetchItem};
use alloc::vec::Vec;
use core::{fmt, ops::Deref};

/// Dereferences the fetch item
pub struct AsDeref<F>(pub(crate) F);

impl<F, V> FetchItem for AsDeref<F>
where
    F: for<'q> FetchItem<Item<'q> = &'q V>,
    V: 'static + Deref,
{
    type Item<'q> = &'q V::Target;
}

impl<'w, F, V> Fetch<'w> for AsDeref<F>
where
    F: Fetch<'w>,
    F: for<'q> FetchItem<Item<'q> = &'q V>,
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

impl<F, V> PreparedFetch for AsDeref<F>
where
    for<'q> F: PreparedFetch<Item<'q> = &'q V> + 'q,
    V: 'static + Deref,
{
    type Item<'q> = &'q V::Target where Self: 'q;

    unsafe fn fetch<'q>(&'q mut self, slot: usize) -> Self::Item<'q> {
        self.0.fetch(slot)
    }

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.0.filter_slots(slots)
    }

    fn set_visited(&mut self, slots: crate::archetype::Slice) {
        self.0.set_visited(slots)
    }
}

impl<F, V> ReadOnlyFetch for AsDeref<F>
where
    for<'q> F: ReadOnlyFetch<Item<'q> = &'q V> + 'q,
    V: 'static + Deref,
{
    unsafe fn fetch_shared<'q>(&self, slot: crate::archetype::Slot) -> Self::Item<'q> {
        self.0.fetch_shared(slot)
    }
}
