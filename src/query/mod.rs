use std::{
    iter::FusedIterator,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    slice::Iter,
};

use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    entity::EntityLocation,
    fetch::{Fetch, PreparedFetch},
    All, Entity, Filter, FilterIter, PrepareInfo, PreparedFilter, World,
};

/// Represents a query and state for a given world.
/// The archetypes to visit is cached in the query which means it is more
/// performant to reuse the query than creating a new one.
///
/// The archetype borrowing assures aliasing.
/// Two of the same queries can be run at the same time as long as they don't
/// borrow an archetype's component mutably at the same time.
pub struct Query<Q> {
    // The archetypes to visit
    archetypes: Vec<ArchetypeId>,
    change_tick: u32,
    archetype_gen: u32,
    fetch: Q,
}

impl<Q> Query<Q>
where
    Q: for<'x> Fetch<'x>,
{
    /// Construct a new query which will fetch all items in the given query.

    /// The query can be either a singular component, a tuple of components, or
    /// any other type which implements [crate::Fetch].
    pub fn new(query: Q) -> Self {
        Self {
            archetypes: Vec::new(),
            fetch: query,
            change_tick: 0,
            archetype_gen: 0,
        }
    }

    /// Execute the query on the world.
    pub fn iter<'a>(&'a mut self, world: &'a World) -> QueryIter<'a, Q> {
        let change_tick = self.change_tick;
        let (archetypes, fetch) = self.get_archetypes(world);

        QueryIter {
            new_tick: if Q::MUTABLE {
                world.advance_change_tick()
            } else {
                0
            },
            archetypes: archetypes.into_iter(),
            current: None,
            fetch,
            world,
        }
    }

    /// Execute the query for a single entity.
    /// A mutable query will advance the global change tick of the world.
    pub fn get<'a>(
        &'a self,
        entity: Entity,
        world: &'a World,
    ) -> Option<QueryBorrow<'a, <Q as Fetch<'_>>::Prepared>> {
        let &EntityLocation { archetype, slot } = world.location(entity)?;

        let archetype = world.archetype(archetype);

        let info = PrepareInfo {
            old_tick: self.change_tick,
            new_tick: self.change_tick,
            slots: Slice::new(slot, slot),
        };

        let mut fetch = self.fetch.prepare(archetype)?;

        // It is only necessary to acquire a new change tick if the query will
        // change anything
        let new_tick = if Q::MUTABLE {
            world.advance_change_tick()
        } else {
            world.change_tick()
        };

        fetch.set_visited(Slice::new(slot, slot), new_tick);

        // Aliasing is guaranteed due to fetch being prepared and alive for this
        // instance only. The lock is held and causes fetches for the same
        // archetype to fail
        let item = unsafe { fetch.fetch(slot) };

        Some(QueryBorrow {
            item,
            _fetch: fetch,
        })
    }

    fn get_archetypes(&mut self, world: &World) -> (&[ArchetypeId], &Q) {
        let fetch = &self.fetch;
        if world.archetype_gen() > self.archetype_gen {
            self.archetypes.clear();
            self.archetypes
                .extend(world.archetypes().filter_map(|(id, arch)| {
                    if fetch.matches(arch) {
                        Some(id)
                    } else {
                        None
                    }
                }))
        }

        (&self.archetypes, fetch)
    }
}

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

pub struct QueryBorrow<'a, F: PreparedFetch<'a>> {
    item: F::Item,
    /// Ensures the borrow is not freed
    _fetch: F,
}

impl<'a, F: PreparedFetch<'a>> Deref for QueryBorrow<'a, F> {
    type Target = F::Item;

    fn deref(&self) -> &Self::Target {
        &self.item
    }
}

impl<'a, F: PreparedFetch<'a>> DerefMut for QueryBorrow<'a, F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.item
    }
}
