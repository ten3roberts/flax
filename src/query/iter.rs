use std::{iter::FusedIterator, slice::Iter};

use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    Fetch, Filter, FilterIter, PreparedFetch, World,
};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct ChunkIter {
    pos: Slot,
    end: Slot,
}

impl ChunkIter {
    pub fn new(slice: Slice) -> Self {
        Self {
            pos: slice.start,
            end: slice.end,
        }
    }

    fn next<'a, F>(&mut self, fetch: &mut F) -> Option<F::Item>
    where
        F: PreparedFetch<'a>,
    {
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
    current_chunk: Option<ChunkIter>,
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

impl<'a, 'b, Q, F> ArchetypeIter<'a, Q, F>
where
    F: Filter<'a>,
    Q: Fetch<'a>,
{
    fn next(&'b mut self) -> Option<Q::Item> {
        loop {
            if let Some(ref mut chunk) = self.current_chunk {
                let item = chunk.next(&mut self.fetch);
                if let Some(item) = item {
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

pub struct QueryIter<'a, Q, F>
where
    Q: Fetch<'a>,
    F: Filter<'a>,
{
    new_tick: u32,
    old_tick: u32,
    archetypes: Iter<'a, ArchetypeId>,
    world: &'a World,
    current: Option<ArchetypeIter<'a, Q, F>>,
    fetch: &'a Q,
    filter: &'a F,
}

impl<'a, Q, F> QueryIter<'a, Q, F>
where
    Q: Fetch<'a>,
    F: Filter<'a>,
{
    pub fn new(
        world: &'a World,
        archetypes: Iter<'a, ArchetypeId>,
        fetch: &'a Q,
        new_tick: u32,
        old_tick: u32,
        filter: &'a F,
    ) -> Self {
        Self {
            new_tick,
            archetypes,
            world,
            current: None,
            fetch,
            filter,
            old_tick,
        }
    }

    /// Get the query iter's new tick.
    #[must_use]
    pub fn new_tick(&self) -> u32 {
        self.new_tick
    }
}

impl<'a, Q, F> Iterator for QueryIter<'a, Q, F>
where
    Q: Fetch<'a>,
    // for<'x> &'x mut <Q as Fetch<'a>>::Prepared: PreparedFetch<'a, Item = Q::Item>,
    F: Filter<'a>,
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

            let chunks = FilterIter::new(arch.slots(), self.filter.prepare(arch, self.old_tick));

            let fetch = self
                .fetch
                .prepare(self.world, arch)
                .expect("Encountered non matched archetype");

            self.current = Some(ArchetypeIter::new(fetch, self.new_tick, chunks));
        }
    }
}

impl<'a, Q, F> FusedIterator for QueryIter<'a, Q, F>
where
    Q: Fetch<'a>,
    F: Filter<'a>,
{
}
