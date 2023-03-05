use core::{iter::Flatten, slice::IterMut};

use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::PreparedFetch,
    filter::{FilterIter, Filtered},
    Entity, Fetch, PreparedArchetype,
};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct Batch<'q, Q, F> {
    arch: &'q Archetype,
    fetch: &'q mut Filtered<Q, F>,
    pos: Slot,
    end: Slot,
}

impl<'q, Q, F> core::fmt::Debug for Batch<'q, Q, F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Batch")
            .field("pos", &self.pos)
            .field("end", &self.end)
            .finish()
    }
}

impl<'q, Q, F> Batch<'q, Q, F> {
    pub(crate) fn new(arch: &'q Archetype, fetch: &'q mut Filtered<Q, F>, slice: Slice) -> Self {
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

impl<'q, Q, F> Iterator for Batch<'q, Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFetch<'q>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Q::Item> {
        if self.pos == self.end {
            None
        } else {
            let fetch = unsafe { &mut *(self.fetch as *mut Filtered<Q, F>) };
            let item = unsafe { fetch.fetch(self.pos) };
            self.pos += 1;
            Some(item)
        }
    }
}

impl<'q, Q, F> Batch<'q, Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFetch<'q>,
{
    pub(crate) fn next_with_id(&mut self) -> Option<(Entity, Q::Item)> {
        if self.pos == self.end {
            None
        } else {
            let fetch = unsafe { &mut *(self.fetch as *mut Filtered<Q, F>) };
            let item = unsafe { fetch.fetch(self.pos) };
            let id = self.arch.entities[self.pos];
            self.pos += 1;
            Some((id, item))
        }
    }

    pub(crate) fn next_full(&mut self) -> Option<(Slot, Entity, Q::Item)> {
        if self.pos == self.end {
            None
        } else {
            let fetch = unsafe { &mut *(self.fetch as *mut Filtered<Q, F>) };
            let slot = self.pos;
            let item = unsafe { fetch.fetch(slot) };
            let id = self.arch.entities[slot];
            self.pos += 1;

            Some((slot, id, item))
        }
    }
}

/// An iterator over a single archetype which returns chunks.
/// The chunk size is determined by the largest continuous matched entities for
/// filters.
pub struct ArchetypeChunks<'q, Q, F> {
    pub(crate) arch: &'q Archetype,
    pub(crate) iter: FilterIter<&'q mut Filtered<Q, F>>,
}

impl<'q, Q, F> Iterator for ArchetypeChunks<'q, Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFetch<'q>,
{
    type Item = Batch<'q, Q, F>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Get the next chunk
        let chunk = self.iter.next()?;

        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *(self.iter.fetch as *mut Filtered<Q, F>) };

        // Set the chunk as visited
        fetch.set_visited(chunk);
        let chunk = Batch::new(self.arch, fetch, chunk);

        Some(chunk)
    }
}
