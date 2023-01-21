use core::{
    fmt::{self, Formatter},
    ops::Deref,
};

use alloc::vec::Vec;

use crate::{archetype::Archetype, Access, Fetch, FetchItem};

use super::{FetchPrepareData, PreparedFetch};

#[derive(Debug, Clone)]
/// Component which copied the value.
///
/// This is useful as the query item is 'static
/// See [crate::Component::as_mut]
pub struct Copied<F>(pub(crate) F);

impl<'q, F, V> FetchItem<'q> for Copied<F>
where
    F: FetchItem<'q, Item = &'q V>,
    V: 'static,
{
    type Item = V;
}

impl<'w, F, V> Fetch<'w> for Copied<F>
where
    F: Fetch<'w>,
    F: for<'q> FetchItem<'q, Item = &'q V>,
    V: 'static + Copy,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = Copied<F::Prepared>;

    #[inline(always)]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(Copied(self.0.prepare(data)?))
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        self.0.filter_arch(arch)
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
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

impl<'q, F, V> PreparedFetch<'q> for Copied<F>
where
    F: PreparedFetch<'q>,
    F::Item: Deref<Target = V>,
    V: 'static + Copy,
{
    type Item = V;

    #[inline]
    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        *self.0.fetch(slot)
    }

    #[inline]
    fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.0.filter_slots(slots)
    }

    #[inline]
    fn set_visited(&mut self, slots: crate::archetype::Slice, change_tick: u32) {
        self.0.set_visited(slots, change_tick)
    }
}
