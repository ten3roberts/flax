mod borrow;
mod data;
mod dfs;
mod difference;
mod entity;
mod iter;
mod planar;
mod searcher;
mod topo;
mod walk;
pub use walk::{Children, DfsIter, GraphBorrow, GraphQuery, Node};

use core::fmt::Debug;

use crate::{
    archetype::Slot,
    fetch::FmtQuery,
    filter::{All, And, BatchSize, Filtered, With, WithRelation, Without, WithoutRelation},
    system::Access,
    Component, ComponentValue, Entity, Fetch, FetchItem, RelationExt, World,
};
use alloc::vec::Vec;

use self::borrow::QueryBorrowState;
pub(crate) use borrow::*;
pub use data::*;
pub use dfs::*;
pub use entity::EntityBorrow;
pub(crate) use iter::*;
pub use planar::*;
pub use searcher::ArchetypeSearcher;
pub use topo::{Topo, TopoBorrow, TopoIter};

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
    fn access(&self, world: &'w World, fetch: &'w Filtered<Q, F>, dst: &mut Vec<Access>);
}

/// Represents a query and state for a given world.
/// The archetypes to visit is cached in the query which means it is more
/// performant to reuse the query than creating a new one.
///
/// The archetype borrowing assures aliasing.
/// Two of the same queries can be run at the same time as long as they don't
/// borrow an archetype's component mutably at the same time.
#[derive(Clone)]
pub struct Query<Q, F = All, S = Planar> {
    fetch: Filtered<Q, F>,

    change_tick: u32,
    archetype_gen: u32,

    strategy: S,
}

impl<Q: Debug, F: Debug, S: Debug> Debug for Query<Q, F, S>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Query")
            .field("fetch", &FmtQuery(&self.fetch.fetch))
            .field("filter", &FmtQuery(&self.fetch.filter))
            .field("change_tick", &self.change_tick)
            .field("strategy", &self.strategy)
            .finish()
    }
}

impl<Q> Query<Q, All, Planar> {
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
            fetch: Filtered::new(fetch, All, false),
            change_tick: 0,
            strategy: Planar::new(),
            archetype_gen: 0,
        }
    }

    /// Include components in a planar query.
    ///
    /// **Note**: only relevant for the `planar` strategy
    pub fn with_components(mut self) -> Self {
        self.fetch.include_components = true;
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
            archetype_gen: 0,
            strategy,
        }
    }

    /// Transform the query into a query for a single entity
    pub fn entity(self, id: Entity) -> EntityQuery<Q, F>
    where
        Entity: for<'w> QueryStrategy<'w, Q, F>,
    {
        self.with_strategy(id)
    }

    /// Transform the query into a topologically ordered query
    pub fn topo<T: ComponentValue>(self, relation: impl RelationExt<T>) -> Query<Q, F, Topo>
    where
        Topo: for<'w> QueryStrategy<'w, Q, F>,
    {
        self.with_strategy(Topo::new(relation))
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
            fetch: Filtered::new(
                self.fetch.fetch,
                And::new(self.fetch.filter, filter),
                self.fetch.include_components,
            ),
            change_tick: self.change_tick,
            archetype_gen: 0,
            strategy: self.strategy,
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

        let borrow_state = QueryBorrowState {
            old_tick,
            new_tick,
            world,
            fetch: &self.fetch,
        };

        let archetype_gen = world.archetype_gen();
        let dirty = archetype_gen > self.archetype_gen;

        self.archetype_gen = archetype_gen;

        self.strategy.borrow(borrow_state, dirty)
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use crate::{filter::Or, name, Entity, Error, FetchExt, Query};

    use super::*;

    #[test]
    fn changes() {
        component! {
            window_width: f32,
            window_height: f32,
            allow_vsync: bool,

            resources,
        }

        let mut world = World::new();

        Entity::builder()
            .set(window_width(), 800.0)
            .set(window_height(), 600.0)
            .set(allow_vsync(), false)
            // Since `resources` is static, it is not required to spawn it
            .append_to(&mut world, resources())
            .unwrap();

        let mut query = Query::new((window_width(), window_height(), allow_vsync())).filter(Or((
            window_width().modified(),
            window_height().modified(),
            allow_vsync().modified(),
        )));

        assert_eq!(
            query.borrow(&world).get(resources()),
            Ok((&800.0, &600.0, &false))
        );
        world.set(resources(), allow_vsync(), true).unwrap();

        assert_eq!(
            query.borrow(&world).get(resources()),
            Ok((&800.0, &600.0, &true))
        );
        assert!(query.borrow(&world).get(resources()).is_err());
    }

    #[test]
    fn get_disjoint() {
        component! {
            a: i32,
            b: i32,
            c: i32,
        }

        let mut world = World::new();

        let _id = Entity::builder().set(a(), 5).set(b(), 5).spawn(&mut world);

        let _id2 = Entity::builder()
            .set(a(), 3)
            .set(b(), 3)
            .set(c(), 1)
            .spawn(&mut world);

        let _id3 = Entity::builder().set(a(), 7).set(b(), 5).spawn(&mut world);
        let id4 = Entity::builder().set(a(), 7).spawn(&mut world);

        let mut query = Query::new((a().modified(), b(), c().opt()));

        let borrow = query.borrow(&world);

        drop(borrow);

        let mut borrow = query.borrow(&world);

        assert_eq!(
            borrow.get(id4),
            Err(Error::MissingComponent(id4, b().info()))
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

        assert_eq!(query.borrow(&world).get(id), Ok(&"id".into()));
        assert_eq!(query.borrow(&world).get(id2), Ok(&"id2".into()));
        assert_eq!(
            query.borrow(&world).get(a().id()),
            Err(Error::DoesNotMatch(a().id()))
        );

        let mut query = query.with_components();
        assert_eq!(query.borrow(&world).get(a().id()), Ok(&"a".into()));
    }
}
