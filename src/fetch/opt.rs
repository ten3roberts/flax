use core::fmt::{self, Formatter};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::FetchPrepareData,
    fetch::PreparedFetch,
    ComponentValue, Fetch,
};

use super::{FetchAccessData, FetchItem, PeekableFetch};

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct Opt<F>(pub(crate) F);

impl<'q, F: FetchItem<'q>> FetchItem<'q> for Opt<F> {
    type Item = Option<F::Item>;
}

impl<'w, F> Fetch<'w> for Opt<F>
where
    F: Fetch<'w>,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = PreparedOpt<F::Prepared>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Self::Prepared {
        PreparedOpt(self.0.try_prepare(data))
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData) -> Vec<crate::Access> {
        self.0.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("opt ")?;
        self.0.describe(f)
    }
}

#[doc(hidden)]
pub struct PreparedOpt<F>(pub(crate) Option<F>);

impl<'p, F> PeekableFetch<'p> for PreparedOpt<F>
where
    F: PeekableFetch<'p>,
{
    type Peek = Option<F::Peek>;

    unsafe fn peek(&'p self, slot: Slot) -> Self::Peek {
        self.0.as_ref().map(|fetch| fetch.peek(slot))
    }
}

impl<'q, F> PreparedFetch<'q> for PreparedOpt<F>
where
    F: PreparedFetch<'q>,
{
    type Item = Option<F::Item>;

    #[inline]
    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
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
    F: Fetch<'w> + for<'q> FetchItem<'q, Item = &'q V>,
    for<'q> F::Prepared: PreparedFetch<'q, Item = &'q V>,
    V: ComponentValue,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = OptOr<Option<F::Prepared>, &'w V>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Self::Prepared {
        OptOr {
            fetch: self.fetch.try_prepare(data),
            or: &self.or,
        }
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData) -> Vec<crate::Access> {
        self.fetch.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("opt_or(")?;
        self.fetch.describe(f)?;
        f.write_str(")")
    }
}

impl<'q, F: FetchItem<'q, Item = &'q V>, V: ComponentValue> FetchItem<'q> for OptOr<F, V> {
    type Item = &'q V;
}

impl<'q, 'w, F, V> PreparedFetch<'q> for OptOr<Option<F>, &'w V>
where
    F: PreparedFetch<'q, Item = &'q V>,
    V: 'static,
{
    type Item = &'q V;

    unsafe fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
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
