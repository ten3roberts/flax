use core::{mem, ops::Deref};

use crate::{
    archetype::{Archetype, Slice},
    entity::EntityLocation,
    fetch::{FetchPrepareData, PreparedFetch},
    EntityRef, Fetch, FetchItem, World,
};

pub struct QueryOne<'w, Q: Fetch<'w>> {
    prepared: Option<Q::Prepared>,
    loc: EntityLocation,
    // item: <Q::Prepared as PreparedFetch<'static>>::Item,
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

    /// Fetches the query item from the entity, or `None` if the entity does not match the query
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
