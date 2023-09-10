use alloc::vec::Vec;

use crate::{Fetch, FetchItem};

use super::{FetchAccessData, FmtQuery, PreparedFetch};

/// Maps the result of a query to another type on the query level.
///
/// **Note**: Due to limitations in the Rust trait system and lifetimes, the provided function must
/// return `'static` items. This is because same function can't be polymorphic over temporary lifetimes issued when the query is prepared and borrowed. The `.map` iterator circumvents this by binding to an already existing lifetime, but will have to be repeated when iterating, and is more difficult to bake into a larger fetch.
pub struct Map<Q, F> {
    pub(crate) query: Q,
    pub(crate) func: F,
}

impl<'q, Q, F, T> FetchItem<'q> for Map<Q, F>
where
    Q: FetchItem<'q>,
    F: Fn(Q::Item) -> T,
    F: 'static,
{
    type Item = T;
}

impl<'w, Q, F, T> Fetch<'w> for Map<Q, F>
where
    Q: Fetch<'w>,
    // for<> Map<Q::Prepared, &'w F>: PreparedFetch<>,
    F: for<'q> Fn(<Q as FetchItem<'q>>::Item) -> T,
    F: 'static,
    T: 'static,
{
    const MUTABLE: bool = Q::MUTABLE;

    type Prepared = Map<Q::Prepared, &'w F>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(Map {
            query: self.query.prepare(data)?,
            func: &self.func,
        })
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.query.filter_arch(data)
    }

    fn access(&self, data: super::FetchAccessData, dst: &mut Vec<crate::system::Access>) {
        self.query.access(data, dst)
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Map").field(&FmtQuery(&self.query)).finish()
    }
}

impl<'w, 'q, Q, F, T> PreparedFetch<'q> for Map<Q, &'w F>
where
    Q: PreparedFetch<'q>,
    F: Fn(Q::Item) -> T,
    F: 'static,
    T: 'static,
{
    type Item = T;

    type Chunk = (&'q F, Q::Chunk);

    unsafe fn create_chunk(&'q mut self, slots: crate::archetype::Slice) -> Self::Chunk {
        (self.func, self.query.create_chunk(slots))
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        (chunk.0)(Q::fetch_next(&mut chunk.1))
    }

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.query.filter_slots(slots)
    }
}
