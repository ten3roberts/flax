use std::slice::IterMut;

use crate::{
    archetype::{Slice, Slot},
    fetch::PreparedFetch,
    filter::FilterIter,
    Archetype, Fetch, Filter,
};

use super::{FilterWithFetch, PreparedArchetype};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct Batch<'q, Q> {
    arch: &'q Archetype,
    fetch: &'q mut Q,
    pos: Slot,
    end: Slot,
}

impl<'q, Q> std::fmt::Debug for Batch<'q, Q> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Batch")
            .field("arch", &self.arch)
            .field("pos", &self.pos)
            .field("end", &self.end)
            .finish()
    }
}

impl<'q, Q> Batch<'q, Q> {
    pub(crate) fn new(arch: &'q Archetype, fetch: &'q mut Q, slice: Slice) -> Self {
        Self {
            arch,
            fetch,
            pos: slice.start,
            end: slice.end,
        }
    }

    pub(crate) fn slots(&self) -> Slice {
        Slice::new(self.pos, self.end)
    }

    /// Returns the archetype for this batch.
    /// **Note**: The borrow of the fetch is still held and may result in borrow
    /// errors.
    pub fn arch(&self) -> &Archetype {
        self.arch
    }

    /// Returns the number of items which would be yielded by this batch
    pub fn len(&self) -> usize {
        self.slots().len()
    }

    /// Returns true if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.slots().is_empty()
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
pub struct Chunks<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
    'w: 'q,
{
    arch: &'q Archetype,
    fetch: &'q mut Q::Prepared,
    filter: FilterIter<<FilterWithFetch<&'w F, Q::Filter> as Filter<'q>>::Prepared>,
    new_tick: u32,
}

impl<'q, 'w, Q, F> Iterator for Chunks<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
    'w: 'q,
{
    type Item = Batch<'q, Q::Prepared>;

    fn next(&mut self) -> Option<Self::Item> {
        // Get the next chunk
        let chunk = self.filter.next();
        let chunk = chunk?;

        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *(self.fetch as *mut Q::Prepared) };

        // Set the chunk as visited
        unsafe { fetch.set_visited(chunk, self.new_tick) }
        let chunk = Batch::new(self.arch, fetch, chunk);

        Some(chunk)
    }
}

/// The query iterator
pub struct QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
{
    iter: BatchedIter<'q, 'w, Q, F>,
    current: Option<<BatchedIter<'q, 'w, Q, F> as Iterator>::Item>,
}

impl<'q, 'w, Q, F> QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
{
    pub(crate) fn new(iter: BatchedIter<'q, 'w, Q, F>) -> Self {
        Self {
            iter,
            current: None,
        }
    }
}

impl<'w, 'q, Q, F> Iterator for QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut batch) = self.current {
                if let Some(item) = batch.next() {
                    return Some(item);
                }
            }

            self.current = Some(self.iter.next()?);
        }
    }
}

/// An iterator which yield disjoint continuous slices for each mathed archetype
/// and filter predicate.
pub struct BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
    'w: 'q,
{
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
    pub(crate) filter: &'q FilterWithFetch<&'w F, Q::Filter>,
    pub(crate) archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared>>,
    pub(crate) current: Option<Chunks<'q, 'w, Q, F>>,
}

impl<'q, 'w, Q, F> BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
{
    pub(super) fn new(
        old_tick: u32,
        new_tick: u32,
        filter: &'q FilterWithFetch<&'w F, Q::Filter>,
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
    &'w F: Filter<'q>,
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
