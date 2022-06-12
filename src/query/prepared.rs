use std::mem::{self, MaybeUninit};

use itertools::Itertools;
use smallvec::SmallVec;

use crate::{
    archetype::{Slice, Slot},
    Archetype, ArchetypeId, Entity, EntityLocation, Fetch, Filter, FilterIter, PreparedFetch,
    PreparedFilter, World,
};

use super::iter::QueryIter;

pub struct PreparedArchetype<'w, Q> {
    pub(crate) id: ArchetypeId,
    pub(crate) arch: &'w Archetype,
    pub(crate) fetch: Q,
}

/// A lazily prepared query which borrows and hands out chunk iterators for
/// each archetype matched.
pub struct PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    pub(crate) prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared>; 8]>,
    pub(crate) world: &'w World,
    pub(crate) archetypes: &'w [ArchetypeId],
    pub(crate) fetch: &'w Q,
    pub(crate) filter: &'w F,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
    // Iter part
    current: Option<Chunks<F::Prepared>>,
    index: usize,
}

impl<'w, 'q, Q, F> IntoIterator for &'q mut PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    type IntoIter = QueryIter<'w, 'q, Q, F>;

    fn into_iter(self) -> Self::IntoIter {
        // Prepared all archetypes
        self.prepared = self
            .archetypes
            .iter()
            .map(|&v| {
                let arch = self.world.archetype(v);
                PreparedArchetype {
                    id: v,
                    arch,
                    fetch: self
                        .fetch
                        .prepare(self.world, arch)
                        .expect("Mismathed archetype"),
                }
            })
            .collect();

        QueryIter::new(self, self.prepared.iter())
    }
}

impl<'w, Q, F> PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'w>,
{
    pub fn new(
        world: &'w World,
        archetypes: &'w [ArchetypeId],
        fetch: &'w Q,
        filter: &'w F,
        old_tick: u32,
        new_tick: u32,
    ) -> Self {
        Self {
            prepared: SmallVec::new(),
            filter,
            old_tick,
            new_tick,
            world,
            archetypes,
            fetch,
            current: None,
            index: 0,
        }
    }

    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> QueryIter<'w, '_, Q, F>
    where
        'w: 'q,
    {
        self.into_iter()
    }

    fn prepare(&mut self, arch: ArchetypeId) -> Option<usize> {
        let world = self.world;
        let prepared = &mut self.prepared;

        if let Some(idx) = prepared.iter().position(|v| v.id == arch) {
            Some(idx)
        } else {
            let archetype = world.archetype(arch);
            let fetch = self.fetch.prepare(world, archetype)?;

            prepared.push(PreparedArchetype {
                id: arch,
                arch: archetype,
                fetch,
            });

            Some(prepared.len() - 1)
        }
    }

    /// Access any number of entites which are disjoint.
    /// Return None if any `id` is not disjoint.
    pub fn get_disjoint<'q, const C: usize>(
        &'q mut self,
        ids: [Entity; C],
    ) -> Option<[<Q::Prepared as PreparedFetch>::Item; C]> {
        let mut sorted = ids;
        sorted.sort();
        if sorted.windows(C).any(|v| v[0] == v[1]) {
            // Not disjoint
            return None;
        }

        // Prepare all
        let mut idxs = [(0, 0); C];

        for i in 0..C {
            let id = ids[i];
            let &EntityLocation { arch, slot } = self.world.location(id)?;
            idxs[i] = (self.prepare(arch)?, slot);
        }

        // Fetch all
        // All items will be initialized
        let mut items: [_; C] = unsafe { MaybeUninit::uninit().assume_init() };
        for i in 0..C {
            let (idx, slot) = idxs[i];
            items[i] = unsafe { self.prepared[idx].fetch.fetch(slot) };
        }

        Some(items)
    }

    /// Get the fetch items for an entity.
    /// **Note**: Filters are ignored.
    pub fn get(&mut self, id: Entity) -> Option<<Q::Prepared as PreparedFetch>::Item> {
        let &EntityLocation { arch, slot } = self.world.location(id)?;

        let idx = self.prepare(arch)?;
        // Since `self` is a mutable references the borrow checker
        // guarantees this borrow is unique
        let p = &self.prepared[idx];
        let item = unsafe { p.fetch.fetch(slot) };

        Some(item)
    }
}

/// An iterator over a single archetype which returns chunks.
/// The chunk size is determined by the largest continuous matched entities for
/// filters.
pub struct Chunks<F> {
    filter: FilterIter<F>,
    chunk: ChunkIter,
    new_tick: u32,
}

impl<F> Chunks<F>
where
    F: PreparedFilter,
{
    fn next<'q, Q>(&'q mut self, fetch: &'q Q) -> Option<Q::Item>
    where
        Q: PreparedFetch<'q>,
    {
        loop {
            if let Some(item) = self.chunk.next(fetch) {
                return Some(item);
            }

            let chunk = ChunkIter::new(self.filter.next()?);

            self.chunk = chunk;
        }
    }
}

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
