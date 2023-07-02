use core::fmt::{self, Formatter};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::FetchPrepareData,
    fetch::PreparedFetch,
    system::Access,
    ComponentValue, Fetch,
};

use super::{FetchAccessData, FetchItem, ReadOnlyFetch};

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct Opt<F>(pub(crate) F);

impl<F: FetchItem> FetchItem for Opt<F> {
    type Item<'q> = Option<F::Item<'q>>;
}

impl<'w, F> Fetch<'w> for Opt<F>
where
    F: Fetch<'w>,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = PreparedOpt<F::Prepared>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedOpt(self.0.prepare(data)))
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.0.access(data, dst)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("opt ")?;
        self.0.describe(f)
    }
}

#[doc(hidden)]
pub struct PreparedOpt<F>(pub(crate) Option<F>);

impl<'p, F> ReadOnlyFetch for PreparedOpt<F>
where
    F: ReadOnlyFetch,
{
    unsafe fn fetch_shared<'q>(&'q self, slot: Slot) -> Self::Item<'q> {
        self.0.as_ref().map(|fetch| fetch.fetch_shared(slot))
    }
}

impl<F> PreparedFetch for PreparedOpt<F>
where
    F: PreparedFetch,
{
    type Item<'q> = Option<F::Item<'q>> where Self: 'q;

    #[inline]
    unsafe fn fetch<'q>(&mut self, slot: usize) -> Self::Item<'q> {
        self.0.as_mut().map(|fetch| fetch.fetch(slot))
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.0 {
            fetch.filter_slots(slots)
        } else {
            slots
        }
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice) {
        if let Some(fetch) = &mut self.0 {
            fetch.set_visited(slots)
        }
    }
}

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct OptOr<F, V> {
    fetch: F,
    or: V,
}

impl<F, V> OptOr<F, V> {
    pub(crate) fn new(inner: F, or: V) -> Self {
        Self { fetch: inner, or }
    }
}

impl<'w, F, V> Fetch<'w> for OptOr<F, V>
where
    F: Fetch<'w> + for<'q> FetchItem<Item<'q> = &'q V>,
    OptOr<Option<F::Prepared>, &'w V>: PreparedFetch,
    V: ComponentValue,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = OptOr<Option<F::Prepared>, &'w V>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(OptOr {
            fetch: self.fetch.prepare(data),
            or: &self.or,
        })
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.fetch.access(data, dst)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("opt_or(")?;
        self.fetch.describe(f)?;
        f.write_str(")")
    }
}

impl<F: for<'q> FetchItem<Item<'q> = &'q V>, V: ComponentValue> FetchItem for OptOr<F, V> {
    type Item<'q> = &'q V;
}

impl<'w, F, V> PreparedFetch for OptOr<Option<F>, &'w V>
where
    for<'q> F: PreparedFetch<Item<'q> = &'q V> + 'q,
    V: 'static,
{
    type Item<'q> = &'q V where Self: 'q;

    unsafe fn fetch<'q>(&'q mut self, slot: crate::archetype::Slot) -> Self::Item<'q> {
        match self.fetch {
            Some(ref mut v) => v.fetch(slot),
            None => self.or,
        }
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.fetch {
            fetch.filter_slots(slots)
        } else {
            slots
        }
    }

    fn set_visited(&mut self, slots: Slice) {
        if let Some(fetch) = &mut self.fetch {
            fetch.set_visited(slots)
        }
    }
}
