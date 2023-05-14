use alloc::vec::Vec;

use crate::{
    archetype::Slice,
    entity::EntityLocation,
    error::Result,
    fetch::{FetchAccessData, PreparedFetch},
    filter::Filtered,
    system::{Access, AccessKind},
    All, ArchetypeSearcher, Entity, Error, Fetch, PreparedArchetype, QueryStrategy, World,
};

use super::{borrow::QueryBorrowState, difference::find_missing_components};

type State<'w, Q, F> = (
    EntityLocation,
    PreparedArchetype<'w, <Q as Fetch<'w>>::Prepared, <F as Fetch<'w>>::Prepared>,
);

fn state<'w, 'a, Q: Fetch<'w>, F: Fetch<'w>>(
    id: Entity,
    state: &'a QueryBorrowState<'w, Q, F>,
) -> Result<State<'w, Q, F>> {
    let loc = match state.world.location(id) {
        Ok(v) => v,
        Err(_) => return Err(Error::NoSuchEntity(id)),
    };

    let arch = state.world.archetypes.get(loc.arch_id);

    let Some(mut p) = state.prepare_fetch(loc.arch_id, arch) else {
        return match find_missing_components(state.fetch, loc.arch_id, state.world).next() {
            Some(missing) => Err(Error::MissingComponent(id, missing)),
            None => Err(Error::DoesNotMatch(id)),
        }
    };

    // Safety
    // Exclusive access
    if unsafe { p.fetch.filter_slots(Slice::single(loc.slot)) }.is_empty() {
        return Err(Error::Filtered(id));
    }

    Ok((loc, p))
}

impl<'w, Q, F> QueryStrategy<'w, Q, F> for Entity
where
    Q: 'w + Fetch<'w>,
    F: 'w + Fetch<'w>,
{
    type Borrow = EntityBorrow<'w, Q, F>;

    fn borrow(&'w mut self, query_state: QueryBorrowState<'w, Q, F>, _dirty: bool) -> Self::Borrow {
        EntityBorrow {
            prepared: state(*self, &query_state),
        }
    }

    fn access(&self, world: &World, fetch: &Filtered<Q, F>) -> Vec<Access> {
        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);

        let mut result = Vec::new();
        searcher.find_archetypes(&world.archetypes, |arch_id, arch| {
            if !fetch.filter_arch(arch) {
                return;
            }

            let data = FetchAccessData {
                world,
                arch,
                arch_id,
            };

            result.append(&mut fetch.access(data))
        });

        result.push(Access {
            kind: AccessKind::World,
            mutable: false,
        });

        result
    }
}

/// Entity(*Query*)Borrow
///
/// A prepared query for a single entity. Holds the locks for the affected archetype and
/// components.
pub struct EntityBorrow<'w, Q, F = All>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    prepared: Result<State<'w, Q, F>>,
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
            Ok((loc, p)) => {
                // self is a mutable reference, so this is the only reference to the slot
                p.fetch.set_visited(Slice::single(loc.slot));
                unsafe { Ok(p.fetch.fetch(loc.slot)) }
            }
            Err(e) => Err(e.clone()),
        }
    }
}

#[cfg(test)]
mod test {

    use glam::{vec3, Vec3};

    use crate::{component, name, FetchExt, Or, Query, System, World};

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

        let mut query = Query::new((name(), a().opt())).entity(id);
        {
            let mut borrow = query.borrow(&world);
            assert_eq!(borrow.get(), Ok((&"Foo".into(), Some(&5))));
            assert_eq!(borrow.get(), Ok((&"Foo".into(), Some(&5))));
        }

        world.remove(id, a()).unwrap();

        assert_eq!(query.borrow(&world).get(), Ok((&"Foo".into(), None)));

        world.remove(id, name()).unwrap();
        assert_eq!(
            query.borrow(&world).get(),
            Err(Error::MissingComponent(id, name().info()))
        );
        world.set(id, name(), "Bar".into()).unwrap();
        {
            let mut borrow = query.borrow(&world);
            assert_eq!(borrow.get(), Ok((&"Bar".into(), None)));
            assert_eq!(borrow.get(), Ok((&"Bar".into(), None)));
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
    fn entity_query3() {
        component! {
            position: Vec3,
        }

        let mut world = World::new();

        let id = Entity::builder()
            .set(name(), "Foo".into())
            .set(position(), vec3(1.4, 6.4, 1.7))
            .spawn(&mut world);

        let id2 = Entity::builder()
            .set(name(), "Bar".into())
            .spawn(&mut world);

        Entity::builder()
            .set(name(), "Baz".into())
            .spawn(&mut world);

        let mut query = Query::new((name(), position().as_mut()));
        assert!(query.borrow(&world).get(id2).is_err());
        assert_eq!(
            query.borrow(&world).get(id),
            Ok((&"Foo".into(), &mut vec3(1.4, 6.4, 1.7)))
        );

        world.set(id2, position(), vec3(4.8, 4.2, 9.1)).unwrap();

        {
            let mut borrow = query.borrow(&world);
            assert_eq!(
                borrow.get(id2),
                Ok((&"Bar".into(), &mut vec3(4.8, 4.2, 9.1)))
            );

            *borrow.get(id2).unwrap().1 = Vec3::X;
        }

        assert_eq!(world.get(id2, position()).as_deref(), Ok(&Vec3::X));

        let mut system = System::builder()
            .with(Query::new(name()).entity(id2))
            .build(|mut q: EntityBorrow<_>| {
                assert_eq!(q.get(), Ok(&"Bar".into()));
            });

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
