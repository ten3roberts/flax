mod borrow;
mod entity;
pub(crate) mod searcher;
use alloc::vec::Vec;
mod iter;
use core::fmt::{self, Debug};

use atomic_refcell::AtomicRef;
use itertools::Itertools;

use crate::{
    archetype::{Archetype, ArchetypeId, Slot},
    component_info,
    fetch::*,
    filter::*,
    system::{SystemAccess, SystemContext, SystemData},
    util::TupleCloned,
    Access, AccessKind, Component, ComponentValue, FetchItem, World,
};
use crate::{AsBorrow, Entity, RelationExt};

pub use borrow::*;
pub use entity::*;
pub use iter::*;

pub use self::searcher::ArchetypeSearcher;

/// Represents a query and state for a given world.
/// The archetypes to visit is cached in the query which means it is more
/// performant to reuse the query than creating a new one.
///
/// The archetype borrowing assures aliasing.
/// Two of the same queries can be run at the same time as long as they don't
/// borrow an archetype's component mutably at the same time.
#[derive(Clone)]
pub struct Query<Q, F = All> {
    // The archetypes to visit
    archetypes: Vec<ArchetypeId>,
    fetch: Filtered<Q, F>,
    include_components: bool,
    change_tick: u32,
    archetype_gen: u32,
}

impl<Q, F> Debug for Query<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Query")
            .field("fetch", &FmtQuery(&self.fetch.fetch))
            .field("filter", &FmtQuery(&self.fetch.filter))
            .finish()
    }
}

impl<Q> Query<Q, All>
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
            fetch: Filtered::new(query, All),
            change_tick: 0,
            archetype_gen: 0,
            include_components: false,
        }
    }

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
    F: for<'x> Fetch<'x>,
{
    /// Transform the query into a query for a single entity
    pub fn entity(self, id: Entity) -> EntityQuery<Q, F> {
        EntityQuery {
            fetch: self.fetch,
            id,
            change_tick: self.change_tick,
        }
    }

    /// Adds a new filter to the query.
    /// This filter is and:ed with the existing filters.
    pub fn filter<G>(self, filter: G) -> Query<Q, And<F, G>> {
        Query {
            fetch: Filtered::new(self.fetch.fetch, And::new(self.fetch.filter, filter)),
            archetypes: Vec::new(),
            change_tick: self.change_tick,
            archetype_gen: self.archetype_gen,
            include_components: self.include_components,
        }
    }

    /// Limits the size of each batch using [`QueryBorrow::iter_batched`]
    pub fn batch_size(self, size: Slot) -> Query<Q, And<F, BatchSize>> {
        self.filter(BatchSize(size))
    }

    /// Shortcut for filter(with_relation)
    pub fn with_relation<T: ComponentValue>(
        self,
        rel: impl RelationExt<T>,
    ) -> Query<Q, And<F, WithRelation>> {
        self.filter(rel.with_relation())
    }

    /// Shortcut for filter(without_relation)
    pub fn without_relation<T: ComponentValue>(
        self,
        rel: impl RelationExt<T>,
    ) -> Query<Q, And<F, WithoutRelation>> {
        self.filter(rel.without_relation())
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
    /// references from iteration or using [QueryBorrow::get`] will have a
    /// lifetime of the [`QueryBorrow`].
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
            self.archetypes = self.get_archetypes(world);
            self.archetype_gen = archetype_gen;
        }

        QueryBorrow::new(world, &self.archetypes, &self.fetch, old_tick, new_tick)
    }

    /// Gathers all elements in the query as a Vec of owned values.
    pub fn as_vec<'w, C>(&'w mut self, world: &'w World) -> Vec<C>
    where
        for<'q> <Q as FetchItem<'q>>::Item: TupleCloned<Cloned = C>,
    {
        let mut prepared = self.borrow(world);
        prepared
            .iter()
            .map(|v| v.clone_tuple_contents())
            .collect_vec()
    }

    /// Collect all elements in the query into a vector
    pub fn collect_vec<'w, T>(&'w mut self, world: &'w World) -> Vec<T>
    where
        T: 'static,
        Q: for<'q> FetchItem<'q, Item = T>,
    {
        self.borrow(world).iter().collect()
    }

    pub(crate) fn get_archetypes<'a>(&'a self, world: &'a World) -> Vec<ArchetypeId> {
        let mut searcher = ArchetypeSearcher::default();
        self.fetch.searcher(&mut searcher);

        if !self.include_components {
            searcher.add_excluded(component_info().key());
        }

        let archetypes = &world.archetypes;

        let filter = |arch: &Archetype| self.fetch.filter_arch(arch);

        let mut result = Vec::new();
        searcher.find_archetypes(archetypes, |arch_id, arch| {
            if filter(arch) {
                result.push(arch_id)
            }
        });

        result
    }
}

impl<Q, F> SystemAccess for Query<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    fn access(&self, world: &World) -> Vec<crate::system::Access> {
        self.get_archetypes(world)
            .into_iter()
            .flat_map(|arch_id| {
                let arch = world.archetypes.get(arch_id);
                let data = FetchAccessData {
                    world,
                    arch,
                    arch_id,
                };

                self.fetch.access(data)
            })
            .chain([Access {
                kind: AccessKind::World,
                mutable: false,
            }])
            .collect_vec()
    }
}

/// Provides a query and a borrow of the world during system execution
pub struct QueryData<'a, Q, F = All>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    world: AtomicRef<'a, World>,
    query: &'a mut Query<Q, F>,
}

impl<'a, Q, F> SystemData<'a> for Query<Q, F>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    type Value = QueryData<'a, Q, F>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        let world = ctx.world().map_err(|_| {
            eyre::eyre!(alloc::format!(
                "Failed to borrow world for query: {:?}",
                self
            ))
        })?;

        Ok(QueryData { world, query: self })
    }
}

impl<'a, Q, F> QueryData<'a, Q, F>
where
    for<'x> Q: Fetch<'x>,
    for<'x> F: Fetch<'x>,
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

impl<'a, 'w, Q, F> AsBorrow<'a> for QueryData<'w, Q, F>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    type Borrowed = QueryBorrow<'a, Q, F>;

    fn as_borrow(&'a mut self) -> Self::Borrowed {
        self.borrow()
    }
}

#[cfg(test)]
mod test {

    use glam::{vec3, Vec3};
    use pretty_assertions::assert_eq;

    use crate::{component, name, Error, Query, System};

    use super::*;

    component! {
        position: Vec3,
    }

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

        let id = Entity::builder().set(a(), 5).set(b(), 5).spawn(&mut world);

        let id2 = Entity::builder()
            .set(a(), 3)
            .set(b(), 3)
            .set(c(), 1)
            .spawn(&mut world);

        let id3 = Entity::builder().set(a(), 7).set(b(), 5).spawn(&mut world);
        let id4 = Entity::builder().set(a(), 7).spawn(&mut world);

        let mut query = Query::new((a().modified(), b(), c().opt()));

        let mut borrow = query.borrow(&world);

        let ids = [id, id2, id3];
        assert_eq!(
            borrow.get_disjoint(ids),
            Ok([(&5, &5, None), (&3, &3, Some(&1)), (&7, &5, None)])
        );

        // Again
        assert_eq!(
            borrow.get_disjoint(ids),
            Ok([(&5, &5, None), (&3, &3, Some(&1)), (&7, &5, None)])
        );

        drop(borrow);
        let mut borrow = query.borrow(&world);
        assert_eq!(borrow.get_disjoint(ids), Err(Error::Filtered(id)));

        assert_eq!(
            borrow.get(id4),
            Err(Error::MissingComponent(id4, b().info()))
        );
    }
}
