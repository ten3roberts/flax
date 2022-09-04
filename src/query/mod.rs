mod borrow;
mod iter;

use std::fmt::Debug;

use atomic_refcell::AtomicRef;
use itertools::Itertools;

use crate::{
    archetype::ArchetypeId,
    fetch::*,
    filter::*,
    is_component,
    system::{SystemAccess, SystemContext, SystemData},
    util::TupleCloned,
    Access, AccessKind, Component, ComponentValue, FetchItem, Filter, World,
};

pub use borrow::*;

type FilterWithFetch<F, Q> = And<F, GatedFilter<Q>>;
/// Represents a query and state for a given world.
/// The archetypes to visit is cached in the query which means it is more
/// performant to reuse the query than creating a new one.
///
/// The archetype borrowing assures aliasing.
/// Two of the same queries can be run at the same time as long as they don't
/// borrow an archetype's component mutably at the same time.
#[derive(Clone)]
pub struct Query<Q, F = All>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Filter<'x>,
{
    // The archetypes to visit
    archetypes: Vec<ArchetypeId>,
    filter: F,
    include_components: bool,
    change_tick: u32,
    archetype_gen: u32,
    fetch: Q,
}

impl<Q, F> Debug for Query<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Filter<'x>,
    F: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf = String::new();
        self.fetch.describe(&mut buf).unwrap();

        f.debug_struct("Query")
            .field("fetch", &buf)
            .field("filter", &self.filter)
            .finish()
    }
}

impl<Q> Query<Q>
where
    Q: for<'x> Fetch<'x>,
{
    /// Construct a new query which will fetch all items in the given query.

    /// The query can be either a singular component, a tuple of components, or
    /// any other type which implements [crate::Fetch].
    ///
    /// **Note**: The query will not yield components, as it may not be intended
    /// behaviour since the most common intent is the entities. See
    /// [`Query::with_components`]
    ///
    /// A fetch may also contain filters
    pub fn new(query: Q) -> Self {
        Self {
            archetypes: Vec::new(),
            filter: All,
            fetch: query,
            change_tick: 0,
            archetype_gen: 0,
            include_components: false,
        }
    }
}

impl<Q> Query<Q, All>
where
    Q: for<'x> Fetch<'x>,
{
    /// Include component entities for the query.
    /// The default is to hide components as they are usually not desired during
    /// iteration.
    pub fn with_components(self) -> Self {
        Self {
            include_components: true,
            ..self
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
    pub fn filter<G: for<'x> Filter<'x>>(self, filter: G) -> Query<Q, And<F, G>> {
        Query {
            filter: And::new(self.filter, filter),
            archetypes: Vec::new(),
            change_tick: self.change_tick,
            archetype_gen: self.archetype_gen,
            fetch: self.fetch,
            include_components: self.include_components,
        }
    }

    /// Shortcut for filter(without)
    pub fn without<T: ComponentValue>(self, component: Component<T>) -> Query<Q, And<F, Without>> {
        self.filter(component.without())
    }

    /// Shortcut for filter(with)
    pub fn with<T: ComponentValue>(self, component: Component<T>) -> Query<Q, And<F, With>> {
        self.filter(component.with())
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
            world.advance_change_tick();
            world.change_tick()
        } else {
            world.change_tick()
        };

        self.change_tick = new_tick;
        (old_tick, new_tick)
    }

    /// Advances and discards all changes up until now.
    /// This has the same effect as iterating and ignoring the results, though
    /// more idiomatic.
    pub fn ignore_changes(&mut self, world: &World) {
        self.change_tick = world.change_tick()
    }

    /// Returns the last change tick the query was run on.
    /// Any changes > change_tick will be yielded in a query iteration.
    pub fn change_tick(&self) -> u32 {
        self.change_tick
    }

    /// Returns true if the query will mutate any components
    pub fn is_mut(&self) -> bool {
        Q::MUTABLE
    }

    /// Borrow the world for the query.
    ///
    /// Allows for both random access and efficient iteration.
    /// See: [`QueryBorrow::get`] and [`QueryBorrow::iter`]
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
    pub fn borrow<'w>(&'w mut self, world: &'w World) -> QueryBorrow<'w, Q, F> {
        let (old_tick, new_tick) = self.prepare_tick(world);

        // Make sure the archetypes to visit are up to date
        let archetype_gen = world.archetype_gen();
        if archetype_gen > self.archetype_gen {
            eprintln!("Recalculating archetypes");
            self.archetypes = self.get_archetypes(world).collect_vec();
            self.archetype_gen = archetype_gen;
        }

        QueryBorrow::new(
            world,
            &self.archetypes,
            &self.fetch,
            &self.filter,
            old_tick,
            new_tick,
        )
    }

    /// Gathers all elements in the query as a Vec of owned values.
    pub fn as_vec<'w, C>(&'w mut self, world: &'w World) -> Vec<C>
    where
        for<'q> <Q as FetchItem<'q>>::Item: TupleCloned<Cloned = C>,
    {
        let mut prepared = self.borrow(world);
        prepared.iter().map(|v| v.cloned()).collect_vec()
    }

    fn get_archetypes<'a>(&'a self, world: &'a World) -> impl Iterator<Item = ArchetypeId> + '_ {
        world.archetypes().filter_map(|(arch_id, arch)| {
            let data = FetchPrepareData {
                world,
                arch,
                arch_id,
            };

            if (self.include_components || !arch.has(is_component().id()))
                && self.fetch.matches(data)
                && self.filter.matches(arch)
                && (!Q::HAS_FILTER || self.fetch.filter().matches(arch))
            {
                Some(arch_id)
            } else {
                None
            }
        })
    }
}

impl<Q, F> SystemAccess for Query<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Filter<'x>,
{
    fn access(&self, world: &World) -> Vec<crate::system::Access> {
        let accesses = self
            .get_archetypes(world)
            .flat_map(|arch_id| {
                let arch = world.archetype(arch_id);
                let data = FetchPrepareData {
                    world,
                    arch,
                    arch_id,
                };
                let mut res = self.fetch.access(data);
                res.append(&mut self.filter.access(arch_id, arch));
                res
            })
            .chain([Access {
                kind: AccessKind::World,
                mutable: false,
            }])
            .collect_vec();

        accesses
    }
}

/// Provides a query and a borrow of the world during system execution
pub struct QueryData<'a, Q, F = All>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Filter<'x> + 'static,
{
    world: AtomicRef<'a, World>,
    query: &'a mut Query<Q, F>,
}

impl<'a, Q, F> SystemData<'a> for Query<Q, F>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Filter<'x> + Debug + 'static,
{
    type Value = QueryData<'a, Q, F>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        let world = ctx
            .world()
            .map_err(|_| eyre::eyre!(format!("Failed to borrow world for query: {:?}", self)))?;

        Ok(QueryData { world, query: self })
    }
}

impl<'a, Q, F> QueryData<'a, Q, F>
where
    for<'x> Q: Fetch<'x>,
    for<'x> F: Filter<'x>,
{
    /// Prepare the query.
    ///
    /// This will borrow all required archetypes for the duration of the
    /// `PreparedQuery`.
    ///
    /// The same query can be prepared multiple times, though not
    /// simultaneously.
    pub fn borrow(&mut self) -> QueryBorrow<Q, F> {
        self.query.borrow(&self.world)
    }
}
