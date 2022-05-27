use std::{iter::FusedIterator, slice::Iter};

use crate::{
    archetype::{ArchetypeId, Slot},
    All, Fetch, Filter, FilterIter, PreparedFetch, World,
};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct ChunkIter<'a, 'q, Q: Fetch<'a>> {
    fetch: &'q Q::Prepared,
    start: Slot,
    pos: Slot,
    end: Slot,
    new_tick: u32,
}

impl<'a, 'q, Q> FusedIterator for ChunkIter<'a, 'q, Q> where Q: Fetch<'a> {}

impl<'a, 'q, Q> Iterator for ChunkIter<'a, 'q, Q>
where
    Q: Fetch<'a>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos == self.end {
            None
        } else {
            let item = unsafe { self.fetch.fetch(self.pos) };
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
    chunks: FilterIter<F::Prepared>,
    new_tick: u32,
}

impl<'a, 'q, Q, F> ArchetypeIter<'a, Q, F>
where
    F: Filter<'a>,
    Q: Fetch<'a>,
{
    fn next(&mut self) -> Option<ChunkIter<'a, '_, Q>> {
        let chunk = self.chunks.next()?;

        // Set the chunk as visited.
        // Has to be done now as the chunk will only have a immutable refernce
        // to the fetch. This is to allow the chunks to be spread across
        // threads. As such, register before multithreading since *this* part is
        // exclusive.
        self.fetch.set_visited(chunk, self.new_tick);

        Some(ChunkIter {
            fetch: &self.fetch,
            start: chunk.start,
            pos: chunk.start,
            end: chunk.end,
            new_tick: self.new_tick,
        })
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
    current: Option<(ArchetypeIter<'a, Q, All>, Option<ChunkIter<'a, 'a, Q>>)>,
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
            match self.current.as_mut() {
                Some((_arch, Some(chunk))) => match chunk.next() {
                    Some(item) => return Some(item),
                    // This chunk is exhausted, request a new chunk in the next
                    // iteration of the loop
                    None => self.current.as_mut().unwrap().1 = None,
                },
                Some((arch, chunk @ None)) => {
                    // Acquire the next chunk from the archetype iterator
                    match arch.next() {
                        Some(new_chunk) => {
                            // Promote the chunks lifetime. The chunk will never be dropped
                            // before the wrapping arch in `current`
                            let new_chunk = ChunkIter {
                                fetch: unsafe { &*(new_chunk.fetch as *const Q::Prepared) },
                                // We cannot use struct update syntax since it links the
                                // lifetime of fetch back to 'q, which we want to avoid
                                start: new_chunk.start,
                                pos: new_chunk.pos,
                                end: new_chunk.end,
                                new_tick: new_chunk.new_tick,
                            };

                            *chunk = Some(new_chunk);
                        }
                        // Current archetype is empty, request a new one
                        None => self.current = None,
                    }
                }
                None => {
                    let &arch = self.archetypes.next()?;
                    let arch = self.world.archetype(arch);

                    let fetch = self
                        .fetch
                        .prepare(arch)
                        .expect("Iterated a non matched archetype");

                    self.current = Some((
                        ArchetypeIter {
                            fetch,
                            chunks: FilterIter::new(arch.slots(), All),
                            new_tick: self.new_tick,
                        },
                        None,
                    ));
                }
            }
        }
    }
}

impl<'a, Q> FusedIterator for QueryIter<'a, Q> where Q: Fetch<'a> {}
