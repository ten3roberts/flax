use core::mem;

use crate::{
    archetype::{Archetype, Slice},
    entity::EntityLocation,
    fetch::{FetchPrepareData, PreparedFetch},
    EntityRef, Fetch, World,
};

pub struct QueryOne<'w, Q: Fetch<'w>> {
    prepared: Q::Prepared,
    item: <Q::Prepared as PreparedFetch<'static>>::Item,
}

impl<'w, Q: Fetch<'w>> QueryOne<'w, Q> {
    pub(crate) fn new(
        fetch: &'w Q,
        world: &'w World,
        arch: &'w Archetype,
        loc: EntityLocation,
    ) -> Option<Self> {
        let mut prepared = fetch.prepare(FetchPrepareData {
            world,
            arch,
            arch_id: loc.arch_id,
            old_tick: 0,
            new_tick: world.advance_change_tick(),
        })?;

        let item = {
            let mut chunk = unsafe { prepared.create_chunk(Slice::single(loc.slot)) };

            unsafe { <Q::Prepared as PreparedFetch<'_>>::fetch_next(&mut chunk) }
        };

        let item = unsafe {
            mem::transmute::<
                <Q::Prepared as PreparedFetch<'_>>::Item,
                <Q::Prepared as PreparedFetch<'static>>::Item,
            >(item)
        };

        Some(Self { prepared, item })
    }
}
