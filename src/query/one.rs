use crate::{
    archetype::{Archetype, Slice},
    entity::EntityLocation,
    fetch::{FetchPrepareData, PreparedFetch},
    Fetch, FetchItem, World,
};

/// Execute a query on a single entity
pub struct QueryOne<'w, Q: Fetch<'w>> {
    prepared: Option<Q::Prepared>,
    loc: EntityLocation,
}

impl<'w, Q: Fetch<'w>> QueryOne<'w, Q> {
    pub(crate) fn new(
        fetch: &'w Q,
        world: &'w World,
        arch: &'w Archetype,
        loc: EntityLocation,
    ) -> Self {
        let prepared = fetch.prepare(FetchPrepareData {
            world,
            arch,
            arch_id: loc.arch_id,
            old_tick: 0,
            new_tick: world.advance_change_tick(),
        });

        Self { prepared, loc }
    }

    /// Fetch the query item from the entity, or `None` if the entity does not match the query
    pub fn get(&mut self) -> Option<<Q as FetchItem<'_>>::Item> {
        match &mut self.prepared {
            Some(prepared) => {
                let item = {
                    let mut chunk = unsafe { prepared.create_chunk(Slice::single(self.loc.slot)) };

                    unsafe { <Q::Prepared as PreparedFetch<'_>>::fetch_next(&mut chunk) }
                };

                Some(item)
            }
            None => None,
        }
    }
}
