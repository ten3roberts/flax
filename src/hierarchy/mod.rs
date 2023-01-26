mod borrow;
mod data;
mod dfs;
mod difference;
mod entity;
mod iter;
mod planar;

pub use planar::{Planar, QueryBorrow};

use crate::archetype::Slot;
use crate::fetch::FmtQuery;
use crate::filter::{BatchSize, Filtered, WithRelation, WithoutRelation};
use crate::{
    Access, And, ArchetypeId, Component, ComponentValue, Entity, FetchItem, RelationExt, With,
    Without,
};
use crate::{All, Fetch, World};

use self::borrow::QueryBorrowState;
pub(crate) use borrow::*;
pub use data::*;
pub use dfs::*;
pub use entity::EntityBorrow;
pub(crate) use iter::*;
pub use planar::*;

/// Similar to [`Query`](crate::Query), except optimized to only fetch a single entity.
///
/// This has the advantage of locking fewer archetypes, and allowing for better multithreading
/// scheduling.
///
/// This replicates the behaviour of [`QueryBorrow::get`](crate::QueryBorrow::get)
///
/// The difference between this and [`EntityRef`](crate::EntityRef) is that the entity ref allows access to any
/// component, wheras the query predeclares a group of components to retrieve. This increases
/// ergonomics in situations such as borrowing resources from a static resource entity.
///
/// Create an entity query using [`Query::entity`](crate::Query::entity).
pub type EntityQuery<Q, F> = Query<Q, F, Entity>;

#[doc(hidden)]
/// Describes how the query behaves and iterates.
pub trait QueryStrategy<'w, Q, F> {
    type Borrow;
    /// Prepare a kind of borrow for the current state
    fn borrow(&'w mut self, query_state: QueryBorrowState<'w, Q, F>, dirty: bool) -> Self::Borrow;

    /// Returns the system access
    fn access(&self, world: &'w World, fetch: &'w Filtered<Q, F>) -> Vec<Access>;
}

// /// Describes how the query behaves and iterates.
// pub trait QueryStrategy {
//     /// Cached state
//     type State<'w>: QueryState<'w>;
//     /// Prepare a state when the world changes shape
//     fn state<'w, Q: Fetch<'w>, F: Fetch<'w>>(
//         &'w self,
//         world: &'w World,
//         fetch: &Filtered<Q, F>,
//     ) -> Self::State<'w>;
// }

// #[doc(hidden)]
// pub trait QueryState<'w> {
//     type Borrow<Q, F>;
//     /// Prepare a kind of borrow for the current state
//     fn borrow<Q: Fetch<'w>, F: Fetch<'w>>(
//         &'w self,
//         query_state: QueryBorrowState<'w, Filtered<Q, F>>,
//     ) -> Self::Borrow<Q, F>;
//     /// Returns the system access
//     fn access<Q: Fetch<'w>, F: Fetch<'w>>(
//         &self,
//         world: &World,
//         fetch: &Filtered<Q, F>,
//     ) -> Vec<Access>;
// }
/// Provides utilities for working with and manipulating hierarchies and graphs
#[derive(Clone)]
pub struct Query<Q, F = All, S = Planar> {
    fetch: Filtered<Q, F>,

    change_tick: u32,
    include_components: bool,
    archetype_gen: u32,

    strategy: S,
}

impl<Q: core::fmt::Debug, F: core::fmt::Debug, S: core::fmt::Debug> core::fmt::Debug
    for Query<Q, F, S>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Query")
            .field("fetch", &FmtQuery(&self.fetch.fetch))
            .field("filter", &FmtQuery(&self.fetch.filter))
            .field("change_tick", &self.change_tick)
            .field("include_components", &self.include_components)
            .field("strategy", &self.strategy)
            .finish()
    }
}

impl<Q> Query<Q, All, Planar>
where
    Planar: for<'x> QueryStrategy<'x, Q, All>,
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
            include_components: false,
            strategy: Planar::new(false),
            archetype_gen: 0,
        }
    }

    /// Include components in a planar query.
    ///
    /// **Note**: only relevant for the `planar` strategy
    pub fn with_components(mut self) -> Self {
        self.strategy.include_components = true;
        self.archetype_gen = 0;
        self
    }
}

impl<Q, F> Query<Q, F, Planar>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    /// Use the given [`QueryStrategy`].
    ///
    /// This replaces the previous strategy
    pub fn with_strategy<S>(self, strategy: S) -> Query<Q, F, S>
    where
        S: for<'w> QueryStrategy<'w, Q, F>,
    {
        Query {
            fetch: self.fetch,
            change_tick: self.change_tick,
            include_components: self.include_components,
            strategy,
            archetype_gen: 0,
        }
    }

    /// Transform the query into a query for a single entity
    pub fn entity(self, id: Entity) -> EntityQuery<Q, F>
    where
        Entity: for<'w> QueryStrategy<'w, Q, F>,
    {
        self.with_strategy(id)
    }

    /// Collect all elements in the query into a vector
    pub fn collect_vec<'w, T>(&'w mut self, world: &'w World) -> Vec<T>
    where
        T: 'static,
        Q: for<'q> FetchItem<'q, Item = T>,
    {
        let mut borrow = self.borrow(world);
        borrow.iter().collect()
    }
}

impl<Q, F, S> Query<Q, F, S>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    /// Adds a new filter to the query.
    /// This filter is and:ed with the existing filters.
    pub fn filter<G>(self, filter: G) -> Query<Q, And<F, G>, S> {
        Query {
            fetch: Filtered::new(self.fetch.fetch, And::new(self.fetch.filter, filter)),
            change_tick: self.change_tick,
            include_components: self.include_components,
            strategy: self.strategy,
            archetype_gen: 0,
        }
    }

    /// Limits the size of each batch using [`QueryBorrow::iter_batched`]
    pub fn batch_size(self, size: Slot) -> Query<Q, And<F, BatchSize>, S> {
        self.filter(BatchSize(size))
    }

    /// Shortcut for filter(with_relation)
    pub fn with_relation<T: ComponentValue>(
        self,
        rel: impl RelationExt<T>,
    ) -> Query<Q, And<F, WithRelation>, S> {
        self.filter(rel.with_relation())
    }

    /// Shortcut for filter(without_relation)
    pub fn without_relation<T: ComponentValue>(
        self,
        rel: impl RelationExt<T>,
    ) -> Query<Q, And<F, WithoutRelation>, S> {
        self.filter(rel.without_relation())
    }

    /// Shortcut for filter(without)
    pub fn without<T: ComponentValue>(
        self,
        component: Component<T>,
    ) -> Query<Q, And<F, Without>, S> {
        self.filter(component.without())
    }

    /// Shortcut for filter(with)
    pub fn with<T: ComponentValue>(self, component: Component<T>) -> Query<Q, And<F, With>, S> {
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
    pub fn borrow<'w>(&'w mut self, world: &'w World) -> S::Borrow
    where
        S: QueryStrategy<'w, Q, F>,
    {
        let (old_tick, new_tick) = self.prepare_tick(world);

        let query_state = QueryBorrowState {
            old_tick,
            new_tick,
            world,
            fetch: &self.fetch,
        };

        let archetype_gen = world.archetype_gen();
        let dirty = archetype_gen > self.archetype_gen;

        self.archetype_gen = archetype_gen;

        self.strategy.borrow(query_state, dirty)
    }

    // pub fn state(&mut self, world: &World) -> &mut S {
    //     let archetype_gen = world.archetype_gen();
    //     if archetype_gen > self.archetype_gen {
    //         self.state = None;
    //         self.archetype_gen = archetype_gen;
    //     }

    //     self.state.get_or_insert_with(|| {
    //         // if !self.include_components {
    //         //     searcher.add_excluded(component_info().key());
    //         // }

    //         self.strategy.state(world, &self.fetch)
    //     })
    // }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{child_of, entity_ids, name, CommandBuffer, Entity, Error, FetchExt, Query};

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
        let mut query =
            Query::new((name().cloned(), a().copied())).with_strategy(Dfs::new(child_of, root));

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

        Query::new((entity_ids(), name()))
            .with_strategy(Dfs::new(child_of, root))
            .borrow(&world)
            .traverse(&Vec::new(), |(id, name), prefix| {
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

        let mut query = Query::new((name().cloned(), a().modified().copied()))
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

    #[test]
    fn test_planar() {
        let mut world = World::new();

        component! {
            a: i32,
        }

        let id = Entity::builder()
            .set(name(), "id".into())
            .set(a(), 5)
            .spawn(&mut world);
        let id2 = Entity::builder()
            .set(name(), "id2".into())
            .set(a(), 7)
            .spawn(&mut world);

        let mut query = Query::new(name());

        assert_eq!(query.borrow(&world).get(id), Ok(&"id".to_string()));
        assert_eq!(query.borrow(&world).get(id2), Ok(&"id2".to_string()));
        assert_eq!(
            query.borrow(&world).get(a().id()),
            Err(Error::DoesNotMatch(a().id()))
        );

        let mut query = query.with_components();
        assert_eq!(query.borrow(&world).get(a().id()), Ok(&"a".to_string()));
    }
}
