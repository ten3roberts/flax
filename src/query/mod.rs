mod iter;
mod view;

use iter::QueryIter;

use std::ops::{Deref, DerefMut};

use crate::{
    archetype::{ArchetypeId, Slice},
    entity::EntityLocation,
    fetch::{Fetch, PreparedFetch},
    Entity, World,
};

/// Represents a query and state for a given world.
/// The archetypes to visit is cached in the query which means it is more
/// performant to reuse the query than creating a new one.
///
/// The archetype borrowing assures aliasing.
/// Two of the same queries can be run at the same time as long as they don't
/// borrow an archetype's component mutably at the same time.
pub struct Query<Q> {
    // The archetypes to visit
    archetypes: Vec<ArchetypeId>,
    change_tick: u32,
    archetype_gen: u32,
    fetch: Q,
}

impl<Q> Query<Q>
where
    Q: for<'x> Fetch<'x>,
{
    /// Construct a new query which will fetch all items in the given query.

    /// The query can be either a singular component, a tuple of components, or
    /// any other type which implements [crate::Fetch].
    pub fn new(query: Q) -> Self {
        Self {
            archetypes: Vec::new(),
            fetch: query,
            change_tick: 0,
            archetype_gen: 0,
        }
    }

    /// Execute the query on the world.
    pub fn iter<'a>(&'a mut self, world: &'a World) -> QueryIter<'a, Q> {
        // Set the change_tick for self to that of the query, to make all
        // changees before this invocation too old
        let change_tick = if Q::MUTABLE {
            world.advance_change_tick()
        } else {
            world.change_tick()
        };

        self.change_tick = change_tick;

        let (archetypes, fetch) = self.get_archetypes(world);

        QueryIter::new(world, archetypes.iter(), fetch, change_tick)
    }

    /// Execute the query for a single entity.
    /// A mutable query will advance the global change tick of the world.
    pub fn get<'a>(
        &'a self,
        entity: Entity,
        world: &'a World,
    ) -> Option<QueryBorrow<'a, <Q as Fetch<'_>>::Prepared>> {
        let &EntityLocation { archetype, slot } = world.location(entity)?;

        let archetype = world.archetype(archetype);

        let mut fetch = self.fetch.prepare(archetype)?;

        // It is only necessary to acquire a new change tick if the query will
        // change anything
        let new_tick = if Q::MUTABLE {
            world.advance_change_tick()
        } else {
            world.change_tick()
        };

        fetch.set_visited(Slice::new(slot, slot), new_tick);

        // Aliasing is guaranteed due to fetch being prepared and alive for this
        // instance only. The lock is held and causes fetches for the same
        // archetype to fail
        let item = unsafe { fetch.fetch(slot) };

        Some(QueryBorrow {
            item,
            _fetch: fetch,
        })
    }

    fn get_archetypes(&mut self, world: &World) -> (&[ArchetypeId], &Q) {
        let fetch = &self.fetch;
        if world.archetype_gen() > self.archetype_gen {
            self.archetypes.clear();
            self.archetypes
                .extend(world.archetypes().filter_map(|(id, arch)| {
                    if fetch.matches(arch) {
                        Some(id)
                    } else {
                        None
                    }
                }))
        }

        (&self.archetypes, fetch)
    }
}
pub struct QueryBorrow<'a, F: PreparedFetch<'a>> {
    item: F::Item,
    /// Ensures the borrow is not freed
    _fetch: F,
}

impl<'a, F: PreparedFetch<'a>> Deref for QueryBorrow<'a, F> {
    type Target = F::Item;

    fn deref(&self) -> &Self::Target {
        &self.item
    }
}

impl<'a, F: PreparedFetch<'a>> DerefMut for QueryBorrow<'a, F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.item
    }
}
