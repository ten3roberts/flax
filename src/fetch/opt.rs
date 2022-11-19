use core::fmt::{self, Formatter};

use alloc::vec::Vec;

use crate::{
    archetype::Archetype, fetch::FetchPrepareData, fetch::PreparedFetch, ComponentKey,
    ComponentValue, Fetch,
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
    const HAS_FILTER: bool = F::HAS_FILTER;
    type Filter = F::Filter;

    type Prepared = PreparedOpt<<F as Fetch<'w>>::Prepared>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedOpt {
            inner: self.0.prepare(data),
        })
    }

    fn matches(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchPrepareData) -> Vec<crate::Access> {
        self.0.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("opt")?;
        self.0.describe(f)
    }

    fn filter(&self) -> Self::Filter {
        self.0.filter()
    }

    fn components(&self, _: &mut Vec<ComponentKey>) {}
}

impl<'q, F: FetchItem<'q>> FetchItem<'q> for Opt<F> {
    type Item = Option<F::Item>;
}

#[doc(hidden)]
pub struct PreparedOpt<F> {
    inner: Option<F>,
}

impl<'q, F> PreparedFetch<'q> for PreparedOpt<F>
where
    F: for<'x> PreparedFetch<'x>,
{
    type Item = Option<<F as PreparedFetch<'q>>::Item>;

    unsafe fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
        self.inner.as_mut().map(|v| v.fetch(slot))
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
    for<'q> <F as Fetch<'w>>::Prepared: PreparedFetch<'q, Item = &'q V>,
    V: ComponentValue,
{
    const MUTABLE: bool = F::MUTABLE;
    const HAS_FILTER: bool = F::HAS_FILTER;
    type Filter = F::Filter;

    type Prepared = PreparedOptOr<'w, <F as Fetch<'w>>::Prepared, V>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedOptOr {
            inner: self.inner.prepare(data),
            or: &self.or,
        })
    }

    fn matches(&self, _: &Archetype) -> bool {
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

    fn filter(&self) -> Self::Filter {
        self.inner.filter()
    }

    fn components(&self, _: &mut Vec<ComponentKey>) {}
}

impl<'q, F: FetchItem<'q, Item = &'q V>, V: ComponentValue> FetchItem<'q> for OptOr<F, V> {
    type Item = &'q V;
}

#[doc(hidden)]
pub struct PreparedOptOr<'w, F, V> {
    inner: Option<F>,
    or: &'w V,
}

impl<'q, 'w, F, V> PreparedFetch<'q> for PreparedOptOr<'w, F, V>
where
    F: for<'x> PreparedFetch<'x, Item = &'x V>,
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
