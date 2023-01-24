use crate::{
    archetype::{unknown_component, Slice},
    entity::EntityLocation,
    error::Result,
    fetch::PreparedFetch,
    Entity, Error, Fetch, PreparedArchetype, QueryState, QueryStrategy,
};

use super::{borrow::QueryBorrowState, difference::find_missing_components};

impl<Q: 'static + for<'x> Fetch<'x>> QueryStrategy<Q> for Entity {
    type State = Entity;

    fn state(&self, _: &crate::World, _fetch: &Q) -> Self::State {
        *self
    }
}

fn state<'w, 'a, Q: Fetch<'w>>(
    id: Entity,
    state: &'a QueryBorrowState<'w, Q>,
) -> EntityState<'w, Q::Prepared> {
    let loc = match state.world.location(id) {
        Ok(v) => v,
        Err(_) => return EntityState::NoSuchEntity(id),
    };

    let arch = state.world.archetypes.get(loc.arch_id);

    // Check static filtering
    if !state.fetch.filter_arch(arch) {
        return EntityState::MismatchedFetch(id, loc);
    }

    let Some(mut p) = state.prepare_fetch(arch, loc.arch_id) else {
        return EntityState::MismatchedFetch(id, loc);
    };

    // Safety
    // Exclusive access
    if unsafe { p.fetch.filter_slots(Slice::single(loc.slot)) }.is_empty() {
        return EntityState::MismatchedFilter(id);
    }

    EntityState::Complete { loc, p }
}

enum EntityState<'w, Q> {
    NoSuchEntity(Entity),
    MismatchedFilter(Entity),
    MismatchedFetch(Entity, EntityLocation),

    Complete {
        loc: EntityLocation,
        p: PreparedArchetype<'w, Q>,
    },
}

impl<'w, Q> QueryState<'w, Q> for Entity
where
    Q: 'w + Fetch<'w>,
{
    type Borrow = EntityBorrow<'w, Q>;

    fn borrow(&'w self, query_state: QueryBorrowState<'w, Q>) -> Self::Borrow {
        EntityBorrow {
            prepared: state(*self, &query_state),
            state: query_state,
        }
    }
}

/// Entity(*Query*)Borrow
///
/// A prepared query for a single entity. Holds the locks for the affected archetype and
/// components.
pub struct EntityBorrow<'w, Q>
where
    Q: Fetch<'w>,
{
    state: QueryBorrowState<'w, Q>,
    prepared: EntityState<'w, Q::Prepared>,
}

impl<'w, Q> EntityBorrow<'w, Q>
where
    Q: Fetch<'w>,
{
    /// Returns the results of the fetch.
    ///
    /// Fails if the entity does not exist, or the fetch isn't matched.
    pub fn get<'q>(&'q mut self) -> Result<<Q::Prepared as PreparedFetch<'q>>::Item>
    where
        'w: 'q,
    {
        match &mut self.prepared {
            EntityState::Complete { loc, p } => {
                // self is a mutable reference, so this is the only reference to the slot
                p.fetch.set_visited(Slice::single(loc.slot));
                unsafe { Ok(p.fetch.fetch(loc.slot)) }
            }
            EntityState::NoSuchEntity(id) => Err(Error::NoSuchEntity(*id)),
            EntityState::MismatchedFilter(id) => Err(Error::MismatchedFilter(*id)),
            EntityState::MismatchedFetch(id, loc) => Err(Error::MissingComponent(
                *id,
                find_missing_components(self.state.fetch, loc.arch_id, self.state.world)
                    .next()
                    .unwrap_or_else(|| unknown_component().info()),
            )),
        }
    }
}

#[cfg(test)]
mod test {

    use glam::{vec3, Vec3};

    use crate::{component, name, FetchExt, GraphQuery, Or, Query, System, World};

    use super::*;

    component! {
        position: Vec3,
        a: i32,
    }

    #[test]
    fn entity_query2() {
        let mut world = World::new();

        let id = Entity::builder()
            .set(name(), "Foo".into())
            .set(a(), 5)
            .spawn(&mut world);

        let mut query = GraphQuery::new((name(), a().opt())).entity(id);
        {
            let mut borrow = query.borrow(&world);
            assert_eq!(borrow.get(), Ok((&"Foo".to_string(), Some(&5))));
            assert_eq!(borrow.get(), Ok((&"Foo".to_string(), Some(&5))));
        }

        world.remove(id, a()).unwrap();

        assert_eq!(query.borrow(&world).get(), Ok((&"Foo".to_string(), None)));

        world.remove(id, name()).unwrap();
        assert_eq!(
            query.borrow(&world).get(),
            Err(Error::MissingComponent(id, name().info()))
        );
        world.set(id, name(), "Bar".into()).unwrap();
        {
            let mut borrow = query.borrow(&world);
            assert_eq!(borrow.get(), Ok((&"Bar".to_string(), None)));
            assert_eq!(borrow.get(), Ok((&"Bar".to_string(), None)));
        }
        world.despawn(id).unwrap();
        assert_eq!(query.borrow(&world).get(), Err(Error::NoSuchEntity(id)));
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

        let mut query = GraphQuery::new((name(), position().as_mut())).entity(id);
        assert!(query.borrow(&world).get().is_err());

        world.set(id, position(), vec3(4.8, 4.2, 9.1)).unwrap();

        {
            let mut borrow = query.borrow(&world);
            assert_eq!(borrow.get(), Ok((&"Bar".into(), &mut vec3(4.8, 4.2, 9.1))));

            *borrow.get().unwrap().1 = Vec3::X;
        }

        assert_eq!(world.get(id, position()).as_deref(), Ok(&Vec3::X));

        //         let mut system = System::builder()
        //             .with(GraphQuery::new(name()).entity(id))
        //             .build(|mut q: EntityBorrow<_>| {
        //                 assert_eq!(q.get(), Ok(&"Bar".into()));
        //             });

        //         system.run_on(&mut world);
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

        let mut query = Query::new((window_width(), window_height(), allow_vsync()))
            .filter(Or((
                window_width().modified(),
                window_height().modified(),
                allow_vsync().modified(),
            )))
            .entity(resources());

        assert_eq!(query.borrow(&world).get(), Ok((&800.0, &600.0, &false)));
        world.set(resources(), allow_vsync(), true).unwrap();

        assert_eq!(query.borrow(&world).get(), Ok((&800.0, &600.0, &true)));
        assert!(query.borrow(&world).get().is_err());
    }
}
