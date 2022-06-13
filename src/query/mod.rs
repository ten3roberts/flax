mod iter;
mod prepared;
mod view;

use itertools::Itertools;

use crate::{
    archetype::ArchetypeId, fetch::Fetch, All, And, Filter, OwnedTuple, PreparedFetch, World,
};

use self::prepared::PreparedQuery;

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

impl<Q> Query<Q, All> {
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
    fn prepare_tick<'w>(&mut self, world: &'w World) -> (u32, u32) {
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

    /// Prepare the query upon the world.
    ///
    /// Allows for both random access and efficient iteration.
    /// See: [`PreparedQuery::get`] and [`PreparedQuery::iter`]
    ///
    /// The returned value holds the borrows of the query fetch. As such, all
    /// references from iteration or using [`PreparedQuery::get`] will have a
    /// lifetime of the [`PreparedQuery`].
    ///
    /// This is because iterators can not yield references to internal state as
    /// all items returned by the iterator need to coexist.
    ///
    /// It is safe to use the same prepared query for both iteration and random
    /// access, Rust's borrow rules will ensure aliasing rules.
    pub fn prepare<'w>(&'w mut self, world: &'w World) -> PreparedQuery<'w, Q, F> {
        let (old_tick, new_tick) = self.prepare_tick(world);
        dbg!(old_tick, new_tick);
        let (archetypes, fetch, filter) = self.get_archetypes(world);

        PreparedQuery::new(world, archetypes, fetch, filter, old_tick, new_tick)
    }

    /// Gathers all elements in the query as a Vec of owned values.
    pub fn as_vec<'w, C>(&'w mut self, world: &'w World) -> Vec<C>
    where
        for<'x, 'y> <<Q as Fetch<'x>>::Prepared as PreparedFetch<'y>>::Item: OwnedTuple<Owned = C>,
    {
        let mut prepared = self.prepare(world);
        let items = prepared.iter().map(|v| v.owned()).collect_vec();
        items
    }

    fn get_archetypes<'w>(&mut self, world: &'w World) -> (&[ArchetypeId], &Q, &F) {
        if world.archetype_gen() > self.archetype_gen {
            self.archetypes.clear();
            self.archetypes
                .extend(world.archetypes().filter_map(|(id, arch)| {
                    if self.fetch.matches(world, arch) {
                        Some(id)
                    } else {
                        None
                    }
                }))
        }

        // Prepare the query

        (&self.archetypes, &self.fetch, &self.filter)
    }
}
