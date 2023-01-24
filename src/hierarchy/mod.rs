mod borrow;
mod dfs;
mod difference;
mod iter;
mod planar;

pub use planar::{Planar, QueryBorrow};

use crate::archetype::Slot;
use crate::filter::{BatchSize, Filtered, WithRelation, WithoutRelation};
use crate::{archetype::Archetype, component_info, All, ArchetypeSearcher, Fetch, World};
use crate::{And, Component, ComponentValue, RelationExt, With, Without};

use self::borrow::QueryBorrowState;
pub(crate) use borrow::*;
pub use dfs::*;
pub(crate) use iter::*;
pub use planar::*;

/// Describes how the query behaves and iterates.
pub trait QueryStrategy<Q> {
    /// Cached state
    type State: for<'x> QueryState<'x, Q>;
    /// Prepare a state when the world changes shape
    fn state<F: Fn(&Archetype) -> bool>(
        &self,
        world: &World,
        searcher: ArchetypeSearcher,
        filter: F,
    ) -> Self::State;
}

#[doc(hidden)]
pub trait QueryState<'w, Q> {
    type Borrow;
    /// Prepare a kind of borrow for the current state
    fn borrow(&'w self, query_state: QueryBorrowState<'w, Q>) -> Self::Borrow;
}

/// Provides utilities for working with and manipulating hierarchies and graphs
pub struct GraphQuery<Q, F, S = Planar>
where
    S: QueryStrategy<Filtered<Q, F>>,
{
    fetch: Filtered<Q, F>,

    change_tick: u32,
    archetype_gen: u32,
    include_components: bool,

    strategy: S,
    state: Option<S::State>,
}

impl<Q> GraphQuery<Q, All, Planar>
where
    Planar: QueryStrategy<Filtered<Q, All>>,
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
    /// Construct a new query which will fetch all items in the given query.

    /// The query can be either a singular component, a tuple of components, or
    /// any other type which implements [crate::Fetch].
    ///
    /// **Note**: The query will not yield components, as it may not be intended
    /// behaviour since the most common intent is the entities. See
    /// [`Query::with_components`]
    ///
    /// A fetch may also contain filters
    /// Construct a new query which will fetch all items in the given query.

    /// The query can be either a singular component, a tuple of components, or
    /// any other type which implements [crate::Fetch].
    ///
    /// **Note**: The query will not yield components, as it may not be intended
    /// behaviour since the most common intent is the entities. See
    /// [`Query::with_components`]
    ///
    /// A fetch may also contain filters
    pub fn new(fetch: Q) -> Self {
        Self {
            fetch: Filtered::new(fetch, All),
            change_tick: 0,
            archetype_gen: 0,
            include_components: false,
            strategy: Planar,
            state: None,
        }
    }
}
impl<Q, F> GraphQuery<Q, F, Planar>
where
    Planar: QueryStrategy<Filtered<Q, F>>,
{
    /// Use the given [`QueryStrategy`]
    pub fn with_strategy<S>(self, strategy: S) -> GraphQuery<Q, F, S>
    where
        S: QueryStrategy<Filtered<Q, F>>,
    {
        GraphQuery {
            fetch: self.fetch,
            change_tick: self.change_tick,
            archetype_gen: self.archetype_gen,
            include_components: self.include_components,
            strategy,
            state: None,
        }
    }
}

impl<Q, F, S> GraphQuery<Q, F, S>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
    S: QueryStrategy<Filtered<Q, F>>,
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

    /// Adds a new filter to the query.
    /// This filter is and:ed with the existing filters.
    pub fn filter<G>(self, filter: G) -> GraphQuery<Q, And<F, G>, S>
    where
        S: QueryStrategy<Filtered<Q, And<F, G>>>,
    {
        GraphQuery {
            fetch: Filtered::new(self.fetch.fetch, And::new(self.fetch.filter, filter)),
            change_tick: self.change_tick,
            archetype_gen: self.archetype_gen,
            include_components: self.include_components,
            strategy: self.strategy,
            state: None,
        }
    }

    /// Limits the size of each batch using [`QueryBorrow::iter_batched`]
    pub fn batch_size(self, size: Slot) -> GraphQuery<Q, And<F, BatchSize>, S>
    where
        S: QueryStrategy<Filtered<Q, And<F, BatchSize>>>,
    {
        self.filter(BatchSize(size))
    }

    /// Shortcut for filter(with_relation)
    pub fn with_relation<T: ComponentValue>(
        self,
        rel: impl RelationExt<T>,
    ) -> GraphQuery<Q, And<F, WithRelation>, S>
    where
        S: QueryStrategy<Filtered<Q, And<F, WithRelation>>>,
    {
        self.filter(rel.with_relation())
    }

    /// Shortcut for filter(without_relation)
    pub fn without_relation<T: ComponentValue>(
        self,
        rel: impl RelationExt<T>,
    ) -> GraphQuery<Q, And<F, WithoutRelation>, S>
    where
        S: QueryStrategy<Filtered<Q, And<F, WithoutRelation>>>,
    {
        self.filter(rel.without_relation())
    }

    /// Shortcut for filter(without)
    pub fn without<T: ComponentValue>(
        self,
        component: Component<T>,
    ) -> GraphQuery<Q, And<F, Without>, S>
    where
        S: QueryStrategy<Filtered<Q, And<F, Without>>>,
    {
        self.filter(component.without())
    }

    /// Shortcut for filter(with)
    pub fn with<T: ComponentValue>(self, component: Component<T>) -> GraphQuery<Q, And<F, With>, S>
    where
        S: QueryStrategy<Filtered<Q, And<F, With>>>,
    {
        self.filter(component.with())
    }

    /// Prepare the next change tick and return the old one for the last time
    /// the query ran
    fn prepare_tick(&mut self, world: &World) -> (u32, u32) {
        // The tick of the last iteration
        let mut old_tick = self.change_tick;

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

        if new_tick < old_tick {
            old_tick = 0;
        }

        self.change_tick = new_tick;
        (old_tick, new_tick)
    }

    /// Borrow the world for the query.
    ///
    /// The returned value holds the borrows of the query fetch. As such, all
    /// references from iteration or using [QueryBorrow::get`] will have a
    /// lifetime of the [`QueryBorrow`].
    ///
    /// This is because iterators can not yield references to internal state as
    /// all items returned by the iterator need to coexist.
    ///
    /// It is safe to use the same prepared query for both iteration and random
    /// access, Rust's borrow rules will ensure aliasing rules.
    pub fn borrow<'w>(
        &'w mut self,
        world: &'w World,
    ) -> <S::State as QueryState<'w, Filtered<Q, F>>>::Borrow
    where
        S::State: QueryState<'w, Filtered<Q, F>>,
    {
        let (old_tick, new_tick) = self.prepare_tick(world);
        // Make sure the archetypes to visit are up to date
        let archetype_gen = world.archetype_gen();
        if archetype_gen > self.archetype_gen {
            self.state = None;
            self.archetype_gen = archetype_gen;
        }

        let state = self.state.get_or_insert_with(|| {
            let mut searcher = ArchetypeSearcher::default();
            self.fetch.searcher(&mut searcher);
            if !self.include_components {
                searcher.add_excluded(component_info().key());
            }

            let filter = |arch: &Archetype| self.fetch.filter_arch(arch);

            self.strategy.state(world, searcher, filter)
        });

        let query_state = QueryBorrowState {
            old_tick,
            new_tick,
            world,
            fetch: &self.fetch,
        };

        state.borrow(query_state)
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{child_of, entity_ids, name, CommandBuffer, Entity, FetchExt, Query};

    use super::*;

    #[test]
    fn traverse_dfs() {
        let mut world = World::new();

        component! {
            a: i32,
            path: String,
            b: &'static str,
        }

        let root = Entity::builder()
            .set(name(), "root".into())
            .set(a(), 0)
            .attach(
                child_of,
                Entity::builder()
                    .set(name(), "child.1".into())
                    .set(a(), 1)
                    .attach(
                        child_of,
                        Entity::builder()
                            .set(name(), "child.1.1".into())
                            .set(a(), 2),
                    ),
            )
            .attach(
                child_of,
                Entity::builder()
                    .set(name(), "child.2".into())
                    .set(a(), 3)
                    .set(b(), "Foo"),
            )
            .attach(
                child_of,
                Entity::builder().set(name(), "child.3".into()).set(a(), 4),
            )
            .spawn(&mut world);

        // let mut query = crate::Query::new((name().cloned(), a().copied()));
        let mut query = GraphQuery::new((name().cloned(), a().copied()))
            .with_strategy(Dfs::new(child_of, root));

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(
            items,
            [
                ("root".to_string(), 0),
                ("child.2".to_string(), 3),
                ("child.1".to_string(), 1),
                ("child.1.1".to_string(), 2),
                ("child.3".to_string(), 4)
            ]
        );

        let mut cmd = CommandBuffer::new();

        GraphQuery::new((entity_ids(), name()))
            .with_strategy(Dfs::new(child_of, root))
            .borrow(&world)
            .cascade(&Vec::new(), |(id, name), prefix| {
                let mut p = prefix.clone();
                p.push(name.clone());

                cmd.set(id, path(), p.join("::"));
                p
            });

        cmd.apply(&mut world).unwrap();

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(
            items,
            [
                ("root".to_string(), 0),
                ("child.1".to_string(), 1),
                ("child.1.1".to_string(), 2),
                ("child.3".to_string(), 4),
                ("child.2".to_string(), 3),
            ]
        );

        let mut paths = Query::new(path().cloned()).collect_vec(&world);
        paths.sort();

        assert_eq!(
            paths,
            [
                "root",
                "root::child.1",
                "root::child.1::child.1.1",
                "root::child.2",
                "root::child.3",
            ]
        );

        // Change detection

        let mut query = GraphQuery::new((name().cloned(), a().modified().copied()))
            .with_strategy(Dfs::new(child_of, root));

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(
            items,
            [
                ("root".to_string(), 0),
                ("child.1".to_string(), 1),
                ("child.1.1".to_string(), 2),
                ("child.3".to_string(), 4),
                ("child.2".to_string(), 3),
            ]
        );

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, []);
        *world.get_mut(root, a()).unwrap() -= 1;
        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, [("root".to_string(), -1)]);

        Query::new((child_of(root), a().as_mut()))
            .borrow(&world)
            .for_each(|(_, a)| {
                *a *= -1;
            });

        // No changes, since the root is not modified
        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, []);

        Query::new((name(), a().as_mut()))
            .filter(child_of(root).with() | name().eq("root".to_string()))
            .borrow(&world)
            .for_each(|(id, a)| {
                eprintln!("Writing to: {id}");
                *a *= -10;
            });

        let items = query.borrow(&world).iter().collect_vec();
        assert_eq!(
            items,
            [
                ("root".to_string(), 10),
                ("child.1".to_string(), 10),
                ("child.3".to_string(), 40),
                ("child.2".to_string(), 30),
            ]
        );
    }
}
