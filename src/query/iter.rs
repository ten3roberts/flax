use std::{iter::Flatten, slice::IterMut};

use crate::{
    archetype::{Slice, Slot},
    Archetype, Fetch, Filter, FilterIter, PreparedFetch, PreparedFilter,
};

use super::prepared::PreparedArchetype;

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct Batch<'q, Q> {
    arch: &'q Archetype,
    fetch: &'q mut Q,
    pos: Slot,
    end: Slot,
}

impl<'q, Q> Batch<'q, Q> {
    pub fn new(arch: &'q Archetype, fetch: &'q mut Q, slice: Slice) -> Self {
        Self {
            arch,
            fetch,
            pos: slice.start,
            end: slice.end,
        }
    }

    pub fn slots(&self) -> Slice {
        Slice::new(self.pos, self.end)
    }

    /// Returns the archetype for this batch.
    /// **Note**: The borrow of the fetch is still held and may result in borrow
    /// errors.
    pub fn arch(&self) -> &Archetype {
        self.arch
    }
}

impl<'q, Q> Iterator for Batch<'q, Q>
where
    Q: PreparedFetch<'q>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Q::Item> {
        if self.pos == self.end {
            None
        } else {
            let fetch = unsafe { &mut *(self.fetch as *mut Q) };
            let item = unsafe { fetch.fetch(self.pos) };
            self.pos += 1;
            Some(item)
        }
    }
}

/// An iterator over a single archetype which returns chunks.
/// The chunk size is determined by the largest continuous matched entities for
/// filters.
pub struct Chunks<'q, Q, F> {
    arch: &'q Archetype,
    fetch: &'q mut Q,
    filter: FilterIter<F>,
    new_tick: u32,
}

impl<'q, Q, F> Iterator for Chunks<'q, Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFilter,
{
    type Item = Batch<'q, Q>;

    fn next(&mut self) -> Option<Self::Item> {
        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *(self.fetch as *mut Q) };

        // Get the next chunk
        let chunk = self.filter.next()?;

        // Set the chunk as visited
        unsafe { fetch.set_visited(chunk, self.new_tick) }
        let chunk = Batch::new(self.arch, fetch, chunk);

        Some(chunk)
    }
}

pub struct QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q, 'w>,
{
    pub(crate) inner: Flatten<BatchedIter<'q, 'w, Q, F>>,
}

impl<'q, 'w, Q, F> QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q, 'w>,
{
    pub fn new(
        old_tick: u32,
        new_tick: u32,
        filter: &'q F,
        archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared>>,
    ) -> Self {
        Self {
            inner: BatchedIter {
                old_tick,
                new_tick,
                filter,
                archetypes,
                current: None,
            }
            .flatten(),
        }
    }
}

impl<'w, 'q, Q, F> Iterator for QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q, 'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// An iterator which yield disjoint continuous slices for each mathed archetype
/// and filter predicate.
pub struct BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q, 'w>,
{
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
    pub(crate) filter: &'q F,
    pub(crate) archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared>>,
    pub(crate) current: Option<Chunks<'q, Q::Prepared, F::Prepared>>,
}

impl<'q, 'w, Q, F> BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q, 'w>,
{
    pub fn new(
        old_tick: u32,
        new_tick: u32,
        filter: &'q F,
        archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared>>,
    ) -> Self {
        Self {
            old_tick,
            new_tick,
            filter,
            archetypes,
            current: None,
        }
    }
}

impl<'w, 'q, Q, F> Iterator for BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q, 'w>,
    'w: 'q,
{
    type Item = Batch<'q, Q::Prepared>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(chunk) = self.current.as_mut() {
                if let item @ Some(..) = chunk.next() {
                    return item;
                }
            }

            let PreparedArchetype { arch, fetch, .. } = self.archetypes.next()?;
            let filter = FilterIter::new(arch.slots(), self.filter.prepare(arch, self.old_tick));

            let chunk = Chunks {
                arch,
                fetch,
                filter,
                new_tick: self.new_tick,
            };

            self.current = Some(chunk);
        }
    }
}
