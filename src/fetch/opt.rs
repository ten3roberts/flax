use core::fmt::{self, Formatter};

use alloc::vec::Vec;
use itertools::Either;

use crate::{
    archetype::{Slice, Slot},
    fetch::FetchPrepareData,
    fetch::PreparedFetch,
    system::Access,
    Fetch,
};

use super::{FetchAccessData, FetchItem, RandomFetch, TransformFetch};

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

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedOpt(self.0.prepare(data)))
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
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

impl<'q, F> RandomFetch<'q> for PreparedOpt<F>
where
    F: RandomFetch<'q>,
{
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
        self.0.as_ref().map(|fetch| fetch.fetch_shared(slot))
    }

    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
        chunk.as_ref().map(|v| F::fetch_shared_chunk(v, slot))
    }
}

impl<'q, F> PreparedFetch<'q> for PreparedOpt<F>
where
    F: PreparedFetch<'q>,
{
    type Item = Option<F::Item>;
    type Chunk = Option<F::Chunk>;

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.0 {
            fetch.filter_slots(slots)
        } else {
            slots
        }
    }

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.0.as_mut().map(|v| v.create_chunk(slots))
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        chunk.as_mut().map(|v| F::fetch_next(v))
    }
}

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct OptOr<F, V> {
    fetch: F,
    value: V,
}

impl<F, V> OptOr<F, V> {
    /// Creates a new `OptOr` fetch modifier
    pub const fn new(inner: F, or: V) -> Self {
        Self {
            fetch: inner,
            value: or,
        }
    }
}

impl<'w, F, V> Fetch<'w> for OptOr<F, V>
where
    F: Fetch<'w> + for<'q> FetchItem<'q, Item = &'q V>,
    V: 'static,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = OptOr<Option<F::Prepared>, &'w V>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(OptOr {
            fetch: self.fetch.prepare(data),
            value: &self.value,
        })
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
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

impl<'q, F: FetchItem<'q, Item = &'q V>, V: 'static> FetchItem<'q> for OptOr<F, V> {
    type Item = &'q V;
}

impl<'w, 'q, F, V> PreparedFetch<'q> for OptOr<Option<F>, &'w V>
where
    F: PreparedFetch<'q, Item = &'q V>,
    V: 'q,
{
    type Item = &'q V;
    type Chunk = Either<F::Chunk, &'q V>;

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.fetch {
            fetch.filter_slots(slots)
        } else {
            slots
        }
    }

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        match self.fetch {
            Some(ref mut v) => Either::Left(v.create_chunk(slots)),
            None => Either::Right(self.value),
        }
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        match chunk {
            Either::Left(v) => F::fetch_next(v),
            Either::Right(v) => v,
        }
    }
}

impl<K, F> TransformFetch<K> for Opt<F>
where
    F: TransformFetch<K>,
{
    type Output = Opt<F::Output>;

    fn transform_fetch(self, method: K) -> Self::Output {
        Opt(self.0.transform_fetch(method))
    }
}

impl<K, F, V> TransformFetch<K> for OptOr<F, V>
where
    F: TransformFetch<K>,
    F: for<'q> FetchItem<'q, Item = &'q V>,
    F::Output: for<'q> FetchItem<'q, Item = &'q V>,
    V: 'static,
{
    type Output = OptOr<F::Output, V>;

    fn transform_fetch(self, method: K) -> Self::Output {
        OptOr {
            fetch: self.fetch.transform_fetch(method),
            value: self.value,
        }
    }
}
