use core::slice;

use smallvec::SmallVec;

use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    Archetype, Fetch, Filter, FilterIter, PreparedFetch, PreparedFilter, World,
};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
#[derive(Default)]
struct ChunkIter {
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

    fn next<'q, Q>(&mut self, fetch: &'q Q) -> Option<Q::Item>
    where
        Q: PreparedFetch<'q>,
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

/*
/// Iterates over an archetype, yielding chunks of entities corresponding to the
/// provided slicing filter.
///
/// # Safety
/// The returned chunks are disjoint, as such, concurrent mutable borrows from
/// the same prepared fetch and atomicref is safe.
pub struct ArchetypeIter<'w, Q: Fetch<'w>, F: Filter<'w>> {
    fetch: Q::Prepared,
    filter: F::Prepared,
    new_tick: u32,
}

impl<'q, 'w, Q, F> IntoIterator for &'q mut ArchetypeIter<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
    &'q mut Q::Prepared: PreparedFetch,
{
    type Item = <&'q mut Q::Prepared as PreparedFetch>::Item;

    type IntoIter = ArchetypesInnerIter<'q, 'w, Q, F>;

    fn into_iter(self) -> Self::IntoIter {
        todo!()
    }
}

pub struct ArchetypesInnerIter<'q, 'w, Q: Fetch<'w>, F: Filter<'w>> {
    fetch: &'q mut Q::Prepared,
    current_chunk: Option<ChunkIter>,
    chunks: FilterIter<F::Prepared>,
    new_tick: u32,
}

impl<'q, 'w, Q, F> Iterator for ArchetypesInnerIter<'q, 'w, Q, F>
where
    F: Filter<'w>,
    Q: Fetch<'w>,
    &'q mut Q::Prepared: PreparedFetch,
{
    type Item = <&'q mut Q::Prepared as PreparedFetch>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut chunk) = self.current_chunk {
                // We know that the returnd references will be distinct
                let fetch: &mut <Q as Fetch>::Prepared = unsafe { &mut *(self.fetch as *mut _) };
                let item = {
                    if chunk.pos == chunk.end {
                        None
                    } else {
                        let item = unsafe { fetch.fetch(chunk.pos) };
                        chunk.pos += 1;
                        Some(item)
                    }
                };

                if let Some(item) = item {
                    return Some(item);
                }
            }

            let chunk = self.chunks.next()?;

            // Mark any changes TODO
            // self.fetch.set_visited(chunk, self.new_tick);

            self.current_chunk = Some(ChunkIter::new(chunk));
        }
    }
}

pub struct QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    world: &'w World,
    new_tick: u32,
    old_tick: u32,
    prepared: &'q mut Vec<(ArchetypeId, Q::Prepared)>,
    filter: &'q F,
    current: Option<ArchetypesInnerIter<'q, 'w, Q, F>>,
    index: usize,
}

impl<'q, 'w, Q, F> Iterator for QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
    &'q mut Q::Prepared: PreparedFetch,
{
    type Item = <&'q mut Q::Prepared as PreparedFetch>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut current) = self.current {
                if let Some(item) = current.next() {
                    return Some(item);
                }
            }

            // Consume current
            self.index += 1;

            let (arch, fetch) = self.prepared.get_mut(self.index)?;
            let fetch: &mut <Q as Fetch>::Prepared = unsafe { &mut *(fetch as *mut _) };
            let arch = self.world.archetype(*arch);
            let chunks = FilterIter::new(arch.slots(), self.filter.prepare(arch, self.old_tick));
            self.current = Some(ArchetypesInnerIter {
                fetch,
                current_chunk: None,
                chunks,
                new_tick: self.new_tick,
            });
        }

        // loop {
        //     if let Some(ref mut chunk) = self.current {
        //         let (filter, fetch) = self.prepared.get_mut(self.index)?;
        //         let fetch: &mut <Q as Fetch>::Prepared = unsafe { &mut *(fetch as *mut _) };
        //         if let Some(item) = {
        //             if chunk.pos == chunk.end {
        //                 // Get a new slice of slots
        //                 if let Some(slots) = filter.ne
        //                 None
        //             } else {
        //                 let item = unsafe { fetch.fetch(chunk.pos) };
        //                 chunk.pos += 1;
        //                 return Some(item);
        //             }
        //         } {
        //             return Some(item);
        //         }
        //     }
        //
        //     self.index += 1;
        //     let next = self.prepared.get(self.index)?;
        // }
    }
} */

// impl<'a, Q, F> Iterator for QueryIter<'a, Q, F>
// where
//     Q: Fetch<'a>,
//     // for<'x> &'x mut <Q as Fetch<'a>>::Prepared: PreparedFetch<'a, Item = Q::Item>,
//     F: Filter<'a>,
// {
//     type Item = Q::Item;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         loop {
//             if let Some(ref mut arch) = self.current {
//                 if let Some(item) = arch.next() {
//                     return Some(item);
//                 }
//             }
//
//             // Get the next archetype
//             let arch = *self.archetypes.next()?;
//             let arch = self.world.archetype(arch);
//
//             let chunks = FilterIter::new(arch.slots(), self.filter.prepare(arch, self.old_tick));
//
//             let fetch = self
//                 .fetch
//                 .prepare(self.world, arch)
//                 .expect("Encountered non matched archetype");
//
//             self.current = Some(ArchetypeIter {
//                 fetch,
//                 current_chunk: None,
//                 chunks,
//                 new_tick: self.new_tick,
//             });
//         }
//     }
// }
//
// impl<'a, Q, F> FusedIterator for QueryIter<'a, Q, F>
// where
//     Q: Fetch<'a>,
//     F: Filter<'a>,
// {
// }

/// An iterator over a single archetype which returns chunks.
/// The chunk size is determined by the largest continuous matched entities for
/// filters.
pub struct Chunks<'q, Q, F> {
    fetch: &'q Q,
    filter: FilterIter<F>,
    chunk: ChunkIter,
    new_tick: u32,
}

impl<'q, Q, F> Iterator for Chunks<'q, Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFilter,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.chunk.next(self.fetch) {
                return Some(item);
            }

            let chunk = ChunkIter::new(self.filter.next()?);

            self.chunk = chunk;
        }
    }
}

struct PreparedState<'w, Q>
where
    Q: Fetch<'w>,
{
    current: Option<Q::Prepared>,
    archetypes: slice::Iter<'w, ArchetypeId>,
}

/// A lazily prepared query which borrows and hands out chunk iterators for
/// each archetype matched.
pub struct PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    state: SmallVec<[(&'w Archetype, Q::Prepared); 8]>,
    filter: &'w F,
    old_tick: u32,
    new_tick: u32,
}

impl<'w, 'q, Q, F> IntoIterator for &'q mut PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    type IntoIter = QueryIter<'w, 'q, Q, F>;

    fn into_iter(self) -> Self::IntoIter {
        QueryIter {
            prepared: self,
            current: None,
            index: 0,
        }
    }
}

impl<'w, Q, F> PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    pub fn new(
        world: &'w World,
        archetypes: impl Iterator<Item = ArchetypeId>,
        fetch: &Q,
        filter: &'w F,
        old_tick: u32,
        new_tick: u32,
    ) -> Self {
        let state = archetypes
            .map(|arch| {
                let arch = world.archetype(arch);
                (
                    arch,
                    fetch
                        .prepare(world, arch)
                        .expect("Attempt to prepare unmatched query"),
                )
            })
            .collect();

        Self {
            state,
            filter,
            old_tick,
            new_tick,
        }
    }
}

impl<'w, Q, F> PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    fn step(&self, current: usize) -> Option<Chunks<'_, Q::Prepared, F::Prepared>> {
        let (arch, fetch) = self.state.get(current)?;

        let filter = FilterIter::new(arch.slots(), self.filter.prepare(arch, self.old_tick));

        Some(Chunks {
            fetch,
            filter,
            new_tick: self.new_tick,
            chunk: Default::default(),
        })
    }
}

pub struct QueryIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    prepared: &'q PreparedQuery<'w, Q, F>,
    current: Option<Chunks<'q, Q::Prepared, F::Prepared>>,
    index: usize,
}

impl<'w, 'q, Q, F> Iterator for QueryIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(chunk) = self.current.as_mut() {
                if let item @ Some(..) = chunk.next() {
                    return item;
                }
            }

            let chunk = self.prepared.step(self.index)?;
            self.index += 1;

            self.current = Some(chunk);
        }
    }
}
