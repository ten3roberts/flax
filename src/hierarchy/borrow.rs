use crate::{
    archetype::Archetype,
    fetch::{FetchPrepareData, PreparedFetch},
    ArchetypeChunks, ArchetypeId, Batch, Entity, Fetch, World,
};

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
    pub fn prepare_fetch(&self, arch: &'w Archetype, arch_id: ArchetypeId) -> Option<Q::Prepared> {
        let data = FetchPrepareData {
            arch,
            arch_id,
            world: self.world,
            old_tick: self.old_tick,
            new_tick: self.new_tick,
        };

        self.fetch.prepare(data)
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
