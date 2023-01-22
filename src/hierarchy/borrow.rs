use crate::{
    archetype::{Archetype, Slice},
    fetch::{FetchPrepareData, PreparedFetch},
    ArchetypeId, Entity, Fetch, World,
};

use super::{ArchetypeChunks, Batch};

pub(crate) struct PreparedArchetype<'w, Q> {
    pub(crate) arch_id: ArchetypeId,
    pub(crate) arch: &'w Archetype,
    pub(crate) fetch: Q,
}

impl<'w, Q> PreparedArchetype<'w, Q> {
    pub fn manual_chunk<'q>(&'q mut self, slots: Slice) -> Option<Batch<'q, Q>>
    where
        Q: PreparedFetch<'q>,
    {
        let chunk = unsafe { self.fetch.filter_slots(slots) };
        if chunk.is_empty() {
            return None;
        }

        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *(&mut self.fetch as *mut Q) };

        // Set the chunk as visited
        fetch.set_visited(chunk);
        let chunk = Batch::new(self.arch, fetch, chunk);
        Some(chunk)
    }

    pub fn chunks(&mut self) -> ArchetypeChunks<Q> {
        ArchetypeChunks {
            slots: self.arch.slots(),
            arch: self.arch,
            fetch: &mut self.fetch,
        }
    }
}

#[doc(hidden)]
pub struct QueryBorrowState<'w, Q> {
    pub(crate) world: &'w World,
    pub(crate) fetch: &'w Q,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
}

impl<'w, Q> QueryBorrowState<'w, Q>
where
    Q: Fetch<'w>,
{
    #[inline]
    pub(crate) fn prepare_fetch(
        &self,
        arch: &'w Archetype,
        arch_id: ArchetypeId,
    ) -> Option<PreparedArchetype<'w, Q::Prepared>> {
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

struct BatchesWithId<'q, Q> {
    chunks: ArchetypeChunks<'q, Q>,
    current: Option<Batch<'q, Q>>,
}

impl<'q, Q> Iterator for BatchesWithId<'q, Q>
where
    Q: PreparedFetch<'q>,
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
