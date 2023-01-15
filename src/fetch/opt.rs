use core::fmt::{self, Formatter};

use alloc::vec::Vec;

use crate::{
    archetype::Archetype, fetch::FetchPrepareData, fetch::PreparedFetch, ComponentValue, Fetch,
};

use super::FetchItem;

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct Opt<F>(pub(crate) F);

impl<F> Opt<F> {}

impl<'w, F> Fetch<'w> for Opt<F>
where
    F: Fetch<'w>,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = Opt<Option<BatchSize>>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<BatchSize> {
        Some(Opt(self.0.prepare(data)))
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchPrepareData) -> Vec<crate::Access> {
        self.0.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("opt ")?;
        self.0.describe(f)
    }

    fn searcher(&self, _: &mut crate::ArchetypeSearcher) {}
}

impl<'q, F: FetchItem<'q>> FetchItem<'q> for Opt<F> {
    type Item = Option<F::Item>;
}

#[doc(hidden)]
pub struct PreparedOpt<F> {
    inner: Option<F>,
}

impl<'q, F> PreparedFetch<'q> for Opt<Option<F>>
where
    F: for<'x> PreparedFetch<'x>,
{
    type Item = Option<<F as PreparedFetch<'q>>::Item>;

    unsafe fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
        self.0.as_mut().map(|v| v.fetch(slot))
    }
}

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct OptOr<F, V> {
    inner: F,
    or: V,
}

impl<F, V> OptOr<F, V> {
    pub(crate) fn new(inner: F, or: V) -> Self {
        Self { inner, or }
    }
}

impl<'w, F, V> Fetch<'w> for OptOr<F, V>
where
    F: Fetch<'w> + for<'q> FetchItem<'q, Item = &'q V>,
    for<'q> BatchSize: PreparedFetch<'q, Item = &'q V>,
    V: ComponentValue,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = OptOr<Option<BatchSize>, &'w V>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<BatchSize> {
        Some(OptOr {
            inner: self.inner.prepare(data),
            or: &self.or,
        })
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchPrepareData) -> Vec<crate::Access> {
        self.inner.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("opt_or(")?;
        self.inner.describe(f)?;
        f.write_str(")")
    }

    fn searcher(&self, _: &mut crate::ArchetypeSearcher) {}
}

impl<'q, F: FetchItem<'q, Item = &'q V>, V: ComponentValue> FetchItem<'q> for OptOr<F, V> {
    type Item = &'q V;
}

#[doc(hidden)]
pub struct PreparedOptOr<'w, F, V> {
    inner: Option<F>,
    or: &'w V,
}

impl<'q, 'w, F, V> PreparedFetch<'q> for OptOr<Option<F>, &'w V>
where
    F: PreparedFetch<'q, Item = &'q V>,
    V: 'static,
{
    type Item = &'q V;

    unsafe fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
        match self.inner {
            Some(ref mut v) => v.fetch(slot),
            None => self.or,
        }
    }
}
