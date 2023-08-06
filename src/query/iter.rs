use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::PreparedFetch,
    filter::{next_slice, Filtered},
    Entity,
};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct Chunk<'q, Q: PreparedFetch<'q>> {
    arch: &'q Archetype,
    fetch: Q::Chunk,
    pos: Slot,
    end: Slot,
}

impl<'q, Q: PreparedFetch<'q>> core::fmt::Debug for Chunk<'q, Q> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Batch")
            .field("pos", &self.pos)
            .field("end", &self.end)
            .finish()
    }
}

impl<'q, Q: PreparedFetch<'q>> Chunk<'q, Q> {
    pub(crate) fn new(arch: &'q Archetype, chunk: Q::Chunk, slice: Slice) -> Self {
        Self {
            arch,
            fetch: chunk,
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

impl<'q, Q> Iterator for Chunk<'q, Q>
where
    Q: PreparedFetch<'q>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Q::Item> {
        if self.pos == self.end {
            None
        } else {
            // let fetch = unsafe { &mut *(self.fetch as *mut Q::Batch) };
            let item = unsafe { Q::fetch_next(&mut self.fetch) };
            self.pos += 1;
            Some(item)
        }
    }
}

impl<'q, Q> Chunk<'q, Q>
where
    Q: PreparedFetch<'q>,
{
    pub(crate) fn next_with_id(&mut self) -> Option<(Entity, Q::Item)> {
        if self.pos == self.end {
            None
        } else {
            let item = unsafe { Q::fetch_next(&mut self.fetch) };
            let id = self.arch.entities[self.pos];
            self.pos += 1;
            Some((id, item))
        }
    }

    pub(crate) fn next_full(&mut self) -> Option<(Slot, Entity, Q::Item)> {
        if self.pos == self.end {
            None
        } else {
            let slot = self.pos;
            let item = unsafe { Q::fetch_next(&mut self.fetch) };
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
    pub(crate) fetch: *mut Filtered<Q, F>,
    pub(crate) slots: Slice,
}

unsafe impl<'q, Q: 'q, F: 'q> Sync for ArchetypeChunks<'q, Q, F> where &'q mut Filtered<Q, F>: Sync {}
unsafe impl<'q, Q: 'q, F: 'q> Send for ArchetypeChunks<'q, Q, F> where &'q mut Filtered<Q, F>: Send {}

impl<'q, Q, F> ArchetypeChunks<'q, Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFetch<'q>,
{
    fn next_slice(slots: &mut Slice, fetch: &mut Filtered<Q, F>) -> Option<Slice> {
        if slots.is_empty() {
            return None;
        }

        while !slots.is_empty() {
            // Safety
            // The yielded slots are split off of `self.slots`
            let cur = unsafe { fetch.filter_slots(*slots) };

            let (_l, m, r) = slots
                .split_with(&cur)
                .expect("Return value of filter must be a subset of `slots");

            assert_eq!(cur, m);

            *slots = r;

            if !m.is_empty() {
                return Some(m);
            }
        }

        None
    }
}

impl<'q, Q, F> Iterator for ArchetypeChunks<'q, Q, F>
where
    Q: 'q + PreparedFetch<'q>,
    F: 'q + PreparedFetch<'q>,
{
    type Item = Chunk<'q, Q>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *self.fetch };

        // Get the next chunk
        let slots = next_slice(&mut self.slots, fetch)?;

        // Disjoing chunk
        let batch = unsafe { fetch.create_chunk(slots) };
        let batch = Chunk::new(self.arch, batch, slots);

        Some(batch)
    }
}
