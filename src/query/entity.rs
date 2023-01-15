use core::fmt::{self};

use atomic_refcell::AtomicRef;

use crate::{
    archetype::{unknown_component, Slice},
    entity::EntityLocation,
    error::Result,
    fetch::{FetchPrepareData, FmtQuery, PreparedFetch},
    filter::Filtered,
    find_missing_components, Access, AccessKind, All, AsBorrow, Entity, Error, Fetch, SystemAccess,
    SystemContext, SystemData, World,
};

#[derive(Clone)]
/// Similar to [`Query`](crate::Query), except optimized to only fetch a single entity.
///
/// This has the advantage of locking fewer archetypes, and allowing for better multithreaded
/// scheduling.
///
/// This replicates the behaviour of [`QueryBorrow::get`](crate::QueryBorrow::get)
///
/// The difference between this and [`EntityRef`](crate::EntityRef) is that the entity ref allows access to any
/// component, wheras the query predeclares a group of components to retrieve. This increases
/// ergonomics in situations such as borrowing resources from a static resource entity.
///
/// Create an entity query using [`Query::entity`](crate::Query::entity).
pub struct EntityQuery<Q, F = All> {
    pub(super) fetch: Filtered<Q, F>,
    pub(super) id: Entity,
    pub(super) change_tick: u32,
}

impl<Q, F> EntityQuery<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
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

    fn state<'w>(
        &'w mut self,
        world: &'w World,
        old_tick: u32,
        new_tick: u32,
    ) -> (
        State<Filtered<<Q as Fetch<'w>>::Prepared, <F as Fetch<'w>>::Prepared>>,
        &Filtered<Q, F>,
    ) {
        let loc = match world.location(self.id) {
            Ok(v) => v,
            Err(_) => return (State::NoSuchEntity(self.id), &self.fetch),
        };

        let arch = world.archetypes.get(loc.arch_id);

        // Check static filtering
        if !self.fetch.filter_arch(arch) {
            return (State::MismatchedFilter(self.id, loc), &self.fetch);
        }

        // Prepare the filter and check for dynamic filtering, for example modification filters
        let data = FetchPrepareData {
            world,
            arch,
            arch_id: loc.arch_id,
            old_tick,
            new_tick,
        };

        match self.fetch.prepare(FetchPrepareData {
            world,
            arch,
            arch_id: loc.arch_id,
            old_tick,
            new_tick,
        }) {
            Some(v) => (State::Complete { loc, prepared: v }, &self.fetch),
            None => (State::MismatchedFetch(self.id, loc), &self.fetch),
        }
    }

    /// Borrow the entity query from the world.
    ///
    /// This locks the components of the entity's archetype.
    ///
    /// **Note**: This operation never fails if the entity does not exist or does not match the
    /// fetch. Instead, the error is returned by [`EntityBorrow::get`].
    pub fn borrow<'w>(&'w mut self, world: &'w World) -> EntityBorrow<'w, Q, F> {
        let (old_tick, new_tick) = self.prepare_tick(world);

        // The entity may not exist, of it may not match the fetch (yet)

        let (state, fetch) = self.state(world, old_tick, new_tick);

        EntityBorrow {
            prepared: state,
            fetch,
            world,
            new_tick,
        }
    }
}

impl<Q, F> core::fmt::Debug for EntityQuery<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Query")
            .field("fetch", &FmtQuery(&self.fetch.fetch))
            .field("filter", &FmtQuery(&self.fetch.fetch))
            .finish()
    }
}

enum State<Q> {
    NoSuchEntity(Entity),
    MismatchedFilter(Entity, EntityLocation),
    MismatchedFetch(Entity, EntityLocation),

    Complete { loc: EntityLocation, prepared: Q },
}

/// Entity(*Query*)Borrow
///
/// A prepared query for a single entity. Holds the locks for the affected archetype and
/// components.
pub struct EntityBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    world: &'w World,
    prepared: State<Filtered<Q::Prepared, F::Prepared>>,
    fetch: &'w Filtered<Q, F>,
    new_tick: u32,
}

impl<'w, Q, F> EntityBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Returns the results of the fetch.
    ///
    /// Fails if the entity does not exist, or the fetch isn't matched.
    pub fn get<'q>(&'q mut self) -> Result<<Q::Prepared as PreparedFetch<'q>>::Item>
    where
        'w: 'q,
    {
        match &mut self.prepared {
            State::Complete { loc, prepared } => {
                // self is a mutable reference, so this is the only reference to the slot
                unsafe {
                    prepared.set_visited(Slice::single(loc.slot), self.new_tick);
                }
                unsafe { Ok(prepared.fetch(loc.slot)) }
            }
            State::NoSuchEntity(id) => Err(Error::NoSuchEntity(*id)),
            State::MismatchedFilter(id, _) => Err(Error::MismatchedFilter(*id)),
            State::MismatchedFetch(id, loc) => Err(Error::MissingComponent(
                *id,
                find_missing_components(self.fetch, loc.arch_id, self.world)
                    .next()
                    .unwrap_or_else(|| unknown_component().info()),
            )),
        }
    }
}

/// Provides a query and a borrow of the world during system execution
pub struct EntityQueryData<'a, Q, F>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    world: AtomicRef<'a, World>,
    query: &'a mut EntityQuery<Q, F>,
}

impl<'a, Q, F> EntityQueryData<'a, Q, F>
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
    pub fn borrow(&mut self) -> EntityBorrow<Q, F> {
        self.query.borrow(&self.world)
    }
}

impl<'a, 'w, Q, F> AsBorrow<'a> for EntityQueryData<'w, Q, F>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    type Borrowed = EntityBorrow<'a, Q, F>;

    fn as_borrow(&'a mut self) -> Self::Borrowed {
        self.borrow()
    }
}

impl<Q, F> SystemAccess for EntityQuery<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    fn access(&self, world: &World) -> alloc::vec::Vec<crate::system::Access> {
        let loc = world.location(self.id);
        match loc {
            Ok(loc) => {
                let arch = world.archetypes.get(loc.arch_id);
                if self.fetch.filter_arch(arch) {
                    let data = FetchPrepareData {
                        world,
                        arch,
                        arch_id: loc.arch_id,
                        old_tick: 0,
                        new_tick: 0,
                    };

                    let mut res = self.fetch.access(data);

                    res.push(Access {
                        kind: AccessKind::World,
                        mutable: false,
                    });
                    res
                } else {
                    Default::default()
                }
            }
            Err(_) => Default::default(),
        }
    }
}

impl<'a, Q, F> SystemData<'a> for EntityQuery<Q, F>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    type Value = EntityQueryData<'a, Q, F>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        let world = ctx.world().map_err(|_| {
            eyre::eyre!(alloc::format!(
                "Failed to borrow world for query: {:?}",
                self
            ))
        })?;

        Ok(EntityQueryData { world, query: self })
    }
}

#[cfg(test)]
mod test {

    use glam::{vec3, Vec3};

    use crate::{component, name, Query, System};

    use super::*;

    component! {
        position: Vec3,
    }

    #[test]
    fn entity_query() {
        let mut world = World::new();

        Entity::builder()
            .set(name(), "Foo".into())
            .set(position(), vec3(1.4, 6.4, 1.7))
            .spawn(&mut world);

        let id = Entity::builder()
            .set(name(), "Bar".into())
            .spawn(&mut world);

        Entity::builder()
            .set(name(), "Baz".into())
            .spawn(&mut world);

        let mut query = Query::new((name(), position().as_mut())).entity(id);
        assert!(query.borrow(&world).get().is_err());

        world.set(id, position(), vec3(4.8, 4.2, 9.1)).unwrap();

        {
            let mut borrow = query.borrow(&world);
            assert_eq!(borrow.get(), Ok((&"Bar".into(), &mut vec3(4.8, 4.2, 9.1))));

            *borrow.get().unwrap().1 = Vec3::X;
        }

        assert_eq!(world.get(id, position()).as_deref(), Ok(&Vec3::X));

        let mut system = System::builder().with(Query::new(name()).entity(id)).build(
            |mut q: EntityBorrow<_, _>| {
                assert_eq!(q.get(), Ok(&"Bar".into()));
            },
        );

        system.run_on(&mut world);
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

        let mut query = Query::new((
            window_width().modified(),
            window_height().modified(),
            allow_vsync().modified(),
        ))
        .entity(resources());

        assert_eq!(query.borrow(&world).get(), Ok((&800.0, &600.0, &false)));
        world.set(resources(), allow_vsync(), true).unwrap();

        assert_eq!(query.borrow(&world).get(), Ok((&800.0, &600.0, &true)));
        assert!(query.borrow(&world).get().is_err());
    }
}
