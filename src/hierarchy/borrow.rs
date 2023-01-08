use core::{iter::Flatten, mem};

use alloc::collections::BTreeMap;
use smallvec::SmallVec;

use crate::{
    access_info,
    archetype::{Slice, Slot},
    fetch::{FetchPrepareData, PreparedEntities, PreparedFetch},
    filter::{FilterIter, PreparedFilter, RefFilter, TupleOr},
    All, ArchetypeChunks, ArchetypeId, Batch, Entity, EntityIds, Fetch, Filter, FilterWithFetch,
    Nothing, PreparedArchetype, World,
};

pub(crate) struct QueryBorrowState<'w, Q, F>
where
    Q: Fetch<'w>,
{
    pub(crate) filter: FilterWithFetch<RefFilter<'w, F>, Q::Filter>,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
}

struct BatchesWithId<'q, Q, F> {
    chunks: ArchetypeChunks<'q, Q, F>,
    current: Option<Batch<'q, Q>>,
}

impl<'q, Q, F> Iterator for BatchesWithId<'q, Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFilter,
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
