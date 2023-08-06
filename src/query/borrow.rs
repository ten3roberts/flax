use crate::{
    archetype::{Archetype, Slice},
    fetch::{FetchPrepareData, PreparedFetch},
    filter::Filtered,
    ArchetypeId, Entity, Fetch, World,
};

use super::{ArchetypeChunks, Chunk};

pub(crate) struct PreparedArchetype<'w, Q, F> {
    pub(crate) arch_id: ArchetypeId,
    pub(crate) arch: &'w Archetype,
    pub(crate) fetch: Filtered<Q, F>,
}

impl<'w, Q, F> PreparedArchetype<'w, Q, F> {
    #[inline]
    pub fn create_chunk<'q>(&'q mut self, slots: Slice) -> Option<Chunk<'q, Q>>
    where
        Q: PreparedFetch<'q>,
        F: PreparedFetch<'q>,
    {
        let slots = unsafe { self.fetch.filter_slots(slots) };
        if slots.is_empty() {
            return None;
        }

        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *(&mut self.fetch as *mut Filtered<Q, F>) };

        let batch = unsafe { fetch.create_chunk(slots) };

        let batch = Chunk::new(self.arch, batch, slots);
        Some(batch)
    }

    #[inline]
    pub fn chunks(&mut self) -> ArchetypeChunks<Q, F> {
        ArchetypeChunks {
            fetch: &mut self.fetch as *mut _,
            slots: self.arch.slots(),
            arch: self.arch,
        }
    }
}

#[doc(hidden)]
pub struct QueryBorrowState<'w, Q, F> {
    pub(crate) world: &'w World,
    pub(crate) fetch: &'w Filtered<Q, F>,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
}

impl<'w, Q, F> QueryBorrowState<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    #[inline]
    pub(crate) fn prepare_fetch(
        &self,
        arch_id: ArchetypeId,
        arch: &'w Archetype,
    ) -> Option<PreparedArchetype<'w, Q::Prepared, F::Prepared>> {
        let data = FetchPrepareData {
            arch,
            arch_id,
            world: self.world,
            old_tick: self.old_tick,
            new_tick: self.new_tick,
        };

        Some(PreparedArchetype {
            arch_id,
            arch,
            fetch: self.fetch.prepare(data)?,
        })
    }
}

struct BatchesWithId<'q, Q: PreparedFetch<'q>, F> {
    chunks: ArchetypeChunks<'q, Q, F>,
    // The current batch
    current: Option<Chunk<'q, Q>>,
}

impl<'q, Q, F> Iterator for BatchesWithId<'q, Q, F>
where
    Q: 'q + PreparedFetch<'q>,
    F: 'q + PreparedFetch<'q>,
{
    type Item = (Entity, Q::Item);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current) = self.current.as_mut() {
                if let item @ Some(_) = current.next_with_id() {
                    return item;
                }
            }

            self.current = Some(self.chunks.next()?);
        }
    }
}
