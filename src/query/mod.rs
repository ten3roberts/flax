mod iter;
mod view;

use iter::QueryIter;

use std::ops::{Deref, DerefMut};

use crate::{
    archetype::{ArchetypeId, Slice},
    entity::EntityLocation,
    fetch::{Fetch, PreparedFetch},
    All, And, Entity, Filter, World,
};

/// Represents a query and state for a given world.
/// The archetypes to visit is cached in the query which means it is more
/// performant to reuse the query than creating a new one.
///
/// The archetype borrowing assures aliasing.
/// Two of the same queries can be run at the same time as long as they don't
/// borrow an archetype's component mutably at the same time.
pub struct Query<Q, F> {
    // The archetypes to visit
    archetypes: Vec<ArchetypeId>,
    filter: F,
    change_tick: u32,
    archetype_gen: u32,
    fetch: Q,
}

impl<Q> Query<Q, All>
where
    Q: for<'x> Fetch<'x>,
{
    /// Construct a new query which will fetch all items in the given query.

    /// The query can be either a singular component, a tuple of components, or
    /// any other type which implements [crate::Fetch].
    pub fn new(query: Q) -> Self {
        Self {
            archetypes: Vec::new(),
            filter: All,
            fetch: query,
            change_tick: 0,
            archetype_gen: 0,
        }
    }
}

impl<Q, F> Query<Q, F>
where
    Q: for<'x> Fetch<'x>,
    for<'x, 'y> &'x <Q as Fetch<'y>>::Prepared: PreparedFetch,
    F: for<'x> Filter<'x>,
{
    /// Adds a new filter to the query.
    /// This filter is and:ed with the existing filters.
    pub fn filter<'a, G: Filter<'a>>(self, filter: G) -> Query<Q, And<F, G>> {
        Query {
            filter: self.filter.and(filter),
            archetypes: Vec::new(),
            change_tick: self.change_tick,
            archetype_gen: self.archetype_gen,
            fetch: self.fetch,
        }
    }

    /// Prepare the next change tick and return the old one for the last time
    /// the query ran
    fn prepare_tick(&mut self, world: &World) -> (u32, u32) {
        // The tick of the last iteration
        let old_tick = self.change_tick;

        // Set the change_tick for self to that of the query, to make all
        // changes before this invocation too old
        //
        // It is only necessary to acquire a new change tick if the query will
        // change anything
        let new_tick = if Q::MUTABLE {
            world.advance_change_tick()
        } else {
            world.change_tick()
        };

        self.change_tick = new_tick;
        (old_tick, new_tick)
    }

    /// Execute the query on the world.
    /// Any change filters will yield the items changed between this and the
    /// previous query for the same query.
    ///
    /// As a result, the first invocation will yield all entities.
    pub fn iter<'a>(&'a mut self, world: &'a World) -> QueryIter<'a, Q, F> {
        let (old_tick, new_tick) = self.prepare_tick(world);
        dbg!(old_tick, new_tick);
        let (archetypes, fetch, filter) = self.get_archetypes(world);

        QueryIter::new(world, archetypes.iter(), fetch, new_tick, old_tick, filter)
    }

    fn get_archetypes(&mut self, world: &World) -> (&[ArchetypeId], &Q, &F) {
        let fetch = &self.fetch;
        if world.archetype_gen() > self.archetype_gen {
            self.archetypes.clear();
            self.archetypes
                .extend(world.archetypes().filter_map(|(id, arch)| {
                    if fetch.matches(world, arch) {
                        Some(id)
                    } else {
                        None
                    }
                }))
        }

        (&self.archetypes, fetch, &self.filter)
    }
}
