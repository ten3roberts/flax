use crate::{fetch::PreparedFetch, ArchetypeChunks, Batch, Entity, Fetch};

pub(crate) struct QueryBorrowState {
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
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
