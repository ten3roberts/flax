use core::fmt::{self, Formatter};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slice},
    fetch::FetchPrepareData,
    fetch::PreparedFetch,
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

    type Prepared = Opt<Option<F::Prepared>>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
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

    fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
        self.0.as_mut().map(|v| v.fetch(slot))
    }

    fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.0 {
            fetch.filter_slots(slots)
        } else {
            slots
        }
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        if let Some(fetch) = &mut self.0 {
            fetch.set_visited(slots, change_tick)
        }
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
    for<'q> F::Prepared: PreparedFetch<'q, Item = &'q V>,
    V: ComponentValue,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = OptOr<Option<F::Prepared>, &'w V>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
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

    fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
        match self.inner {
            Some(ref mut v) => v.fetch(slot),
            None => self.or,
        }
    }

    fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.inner {
            fetch.filter_slots(slots)
        } else {
            slots
        }
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        if let Some(fetch) = &mut self.inner {
            fetch.set_visited(slots, change_tick)
        }
    }
}
