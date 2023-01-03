use core::fmt::{self, Formatter};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slot},
    Access, Fetch, FetchItem,
};

use super::{FetchPrepareData, PreparedFetch};

#[derive(Debug, Clone)]
/// Component which copied the value.
///
/// This is useful as the query item is 'static
/// See [crate::Component::as_mut]
pub struct Copied<F>(pub(crate) F);

impl<'w, F, V> Fetch<'w> for Copied<F>
where
    F: Fetch<'w> + for<'q> FetchItem<'q, Item = &'q V>,
    for<'q> <F as Fetch<'w>>::Prepared: PreparedFetch<'q, Item = &'q V>,
    V: 'static + Copy,
{
    const MUTABLE: bool = F::MUTABLE;
    const HAS_FILTER: bool = F::HAS_FILTER;

    type Filter = F::Filter;

    type Prepared = Copied<F::Prepared>;

    #[inline(always)]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let inner = self.0.prepare(data)?;

        Some(Copied(inner))
    }

    fn matches(&self, arch: &Archetype) -> bool {
        self.0.matches(arch)
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        self.0.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("clone ")?;
        self.0.describe(f)
    }

    fn filter(&self) -> Self::Filter {
        self.0.filter()
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        self.0.searcher(searcher)
    }
}

impl<'q, F, V> FetchItem<'q> for Copied<F>
where
    F: FetchItem<'q, Item = &'q V>,
    V: 'static + Copy,
{
    type Item = V;
}

impl<'q, F, V> PreparedFetch<'q> for Copied<F>
where
    F: PreparedFetch<'q, Item = &'q V>,
    V: 'q + Copy,
{
    type Item = V;

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        *self.0.fetch(slot)
    }
}
