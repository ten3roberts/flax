use core::{
    fmt::{self, Formatter},
    ops::Deref,
};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slice},
    Access, Fetch, FetchItem,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch, ReadOnlyFetch};

#[derive(Debug, Clone)]
/// Component which cloned the value.
///
/// This is useful as the query item is 'static
/// See [crate::Component::as_mut]
pub struct Cloned<F>(pub(crate) F);

impl<'q, F, V> FetchItem<'q> for Cloned<F>
where
    F: FetchItem<'q, Item = &'q V>,
    V: 'static,
{
    type Item = V;
}

impl<'w, F, V> Fetch<'w> for Cloned<F>
where
    F: Fetch<'w>,
    F: for<'q> FetchItem<'q, Item = &'q V>,
    V: 'static + Clone,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = Cloned<F::Prepared>;

    #[inline(always)]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(Cloned(self.0.prepare(data)?))
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        self.0.filter_arch(arch)
    }

    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        self.0.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("clone ")?;
        self.0.describe(f)
    }

    #[inline]
    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        self.0.searcher(searcher)
    }
}

impl<'q, F, V> PreparedFetch<'q> for Cloned<F>
where
    F: PreparedFetch<'q>,
    F::Item: Deref<Target = V>,
    V: 'static + Clone,
{
    type Item = V;

    #[inline]
    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        self.0.fetch(slot).clone()
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        self.0.filter_slots(slots)
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice) {
        self.0.set_visited(slots)
    }
}

impl<'q, V, F> ReadOnlyFetch<'q> for Cloned<F>
where
    F: ReadOnlyFetch<'q>,
    F::Item: Deref<Target = V>,
    V: 'static + Clone,
{
    unsafe fn fetch_shared(&'q self, slot: crate::archetype::Slot) -> Self::Item {
        self.0.fetch_shared(slot).clone()
    }
}
