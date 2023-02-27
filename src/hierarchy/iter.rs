use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::PreparedFetch,
    Entity,
};

/// Iterates over a chunk of entities, specified by a predicate.
/// In essence, this is the unflattened version of [crate::QueryIter].
pub struct Batch<'q, Q> {
    arch: &'q Archetype,
    fetch: &'q mut Q,
    pos: Slot,
    end: Slot,
}

impl<'q, Q> core::fmt::Debug for Batch<'q, Q> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Batch")
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

impl<'q, Q> Batch<'q, Q>
where
    Q: PreparedFetch<'q>,
{
    pub(crate) fn next_with_id(&mut self) -> Option<(Entity, Q::Item)> {
        if self.pos == self.end {
            None
        } else {
            let fetch = unsafe { &mut *(self.fetch as *mut Q) };
            let item = unsafe { fetch.fetch(self.pos) };
            let id = self.arch.entities[self.pos];
            self.pos += 1;
            Some((id, item))
        }
    }
}

/// An iterator over a single archetype which returns chunks.
/// The chunk size is determined by the largest continuous matched entities for
/// filters.
pub struct ArchetypeChunks<'q, Q> {
    pub(crate) arch: &'q Archetype,
    pub(crate) fetch: &'q mut Q,
    /// The slots which remain to iterate over
    pub(crate) slots: Slice,
}

impl<'q, Q> ArchetypeChunks<'q, Q>
where
    Q: PreparedFetch<'q>,
{
    pub(crate) fn next_chunk(&mut self) -> Option<Slice> {
        if self.slots.is_empty() {
            return None;
        }

        // Safety
        // The yielded slots are split off of `self.slots`
        let cur = unsafe { self.fetch.filter_slots(self.slots) };

        if cur.is_empty() {
            None
        } else {
            let (_l, m, r) = self
                .slots
                .split_with(&cur)
                .expect("Return value of filter must be a subset of `slots");

            self.slots = r;
            Some(m)
        }
    }
}

impl<'q, Q> Iterator for ArchetypeChunks<'q, Q>
where
    Q: PreparedFetch<'q>,
{
    type Item = Batch<'q, Q>;

    fn next(&mut self) -> Option<Self::Item> {
        // Get the next chunk
        let chunk = self.next_chunk()?;

        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *(self.fetch as *mut Q) };

        // Set the chunk as visited
        fetch.set_visited(chunk);
        let chunk = Batch::new(self.arch, fetch, chunk);

        Some(chunk)
    }
}
