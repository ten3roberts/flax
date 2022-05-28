use std::{iter::FusedIterator, marker::PhantomData, slice::Iter};

use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    All, Fetch, Filter, FilterIter, PreparedFetch, World,
};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct ChunkIter<'a, Q: Fetch<'a>> {
    pos: Slot,
    end: Slot,
    _marker: PhantomData<&'a Q>,
}

impl<'a, Q> ChunkIter<'a, Q>
where
    Q: Fetch<'a>,
{
    pub fn new(slice: Slice) -> Self {
        Self {
            pos: slice.start,
            end: slice.end,
            _marker: PhantomData,
        }
    }

    fn next(&mut self, fetch: &mut Q::Prepared) -> Option<Q::Item> {
        if self.pos == self.end {
            None
        } else {
            let item = unsafe { fetch.fetch(self.pos) };
            self.pos += 1;
            Some(item)
        }
    }
}

/// Iterates over an archetype, yielding chunks of entities corresponding to the
/// provided slicing filter.
///
/// # Safety
/// The returned chunks are disjoint, as such, concurrent mutable borrows from
/// the same prepared fetch and atomicref is safe.
pub struct ArchetypeIter<'a, Q: Fetch<'a>, F: Filter<'a>> {
    /// This field will never change, as such it is safe to hand out references
    /// to this fetch as long as self is valid.
    fetch: Q::Prepared,
    current_chunk: Option<ChunkIter<'a, Q>>,
    chunks: FilterIter<F::Prepared>,
    new_tick: u32,
}

impl<'a, Q: Fetch<'a>, F: Filter<'a>> ArchetypeIter<'a, Q, F> {
    fn new(fetch: Q::Prepared, new_tick: u32, chunks: FilterIter<F::Prepared>) -> Self {
        Self {
            fetch,
            current_chunk: None,
            chunks,
            new_tick,
        }
    }
}

impl<'a, 'q, Q, F> ArchetypeIter<'a, Q, F>
where
    F: Filter<'a>,
    Q: Fetch<'a>,
{
    fn next(&mut self) -> Option<Q::Item> {
        loop {
            if let Some(ref mut chunk) = self.current_chunk {
                if let Some(item) = chunk.next(&mut self.fetch) {
                    return Some(item);
                }
            }

            let chunk = self.chunks.next()?;

            // Mark any changes
            self.fetch.set_visited(chunk, self.new_tick);

            self.current_chunk = Some(ChunkIter::new(chunk));
        }
    }
}

pub struct QueryIter<'a, Q>
where
    Q: Fetch<'a>,
{
    new_tick: u32,
    archetypes: Iter<'a, ArchetypeId>,
    world: &'a World,
    /// The lifetime of chunk iter is promoted from <'a, 'q>, where 'q refers to
    /// the `ArchetypeIter`. The archetype iter is held atleast as long as
    /// chunkiter.
    current: Option<ArchetypeIter<'a, Q, All>>,
    fetch: &'a Q,
}

impl<'a, Q> QueryIter<'a, Q>
where
    Q: Fetch<'a>,
{
    pub fn new(
        world: &'a World,
        archetypes: Iter<'a, ArchetypeId>,
        fetch: &'a Q,
        new_tick: u32,
    ) -> Self {
        Self {
            new_tick,
            archetypes,
            world,
            current: None,
            fetch,
        }
    }

    /// Get the query iter's new tick.
    #[must_use]
    pub fn new_tick(&self) -> u32 {
        self.new_tick
    }
}

impl<'a, Q> Iterator for QueryIter<'a, Q>
where
    Q: Fetch<'a>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut arch) = self.current {
                if let Some(item) = arch.next() {
                    return Some(item);
                }
            }

            // Get the next archetype
            let arch = *self.archetypes.next()?;
            let arch = self.world.archetype(arch);

            let fetch = self
                .fetch
                .prepare(arch)
                .expect("Encountered non matched archetype");

            let chunks = FilterIter::new(arch.slots(), All);

            self.current = Some(ArchetypeIter::new(fetch, self.new_tick, chunks));
        }
    }
}

impl<'a, Q> FusedIterator for QueryIter<'a, Q> where Q: Fetch<'a> {}
