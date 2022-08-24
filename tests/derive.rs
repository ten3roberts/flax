use flax_derive::Fetch;
use glam::*;

#[test]
fn derive_fetch() {
    flax::component! {
        position: Vec3 => [flax::Debug],
        rotation: Quat => [flax::Debug],
        scale: Vec3 => [flax::Debug],
    }

    use glam::*;

    use flax::Fetch;
    #[derive(Debug, Clone)]
    struct TransformQuery {
        pos: Component<Vec3>,
        rot: Component<Quat>,
        scale: Component<Vec3>,
    }

    struct Prepared<'w> {
        pos: <Component<Vec3> as Fetch<'w>>::Prepared,
        rot: <Component<Quat> as Fetch<'w>>::Prepared,
        scale: <Component<Vec3> as Fetch<'w>>::Prepared,
    }

    struct Res<'q> {
        pos: <Component<Vec3> as FetchItem<'q>>::Item,
        rot: <Component<Quat> as FetchItem<'q>>::Item,
        scale: <Component<Vec3> as FetchItem<'q>>::Item,
    }

    impl<'w, 'q> PreparedFetch<'q> for Prepared<'w> {
        type Item = Res<'q>;

        unsafe fn fetch(&'q mut self, slot: archetype::Slot) -> Self::Item {
            Self::Item {
                pos: self.pos.fetch(slot),
                rot: self.rot.fetch(slot),
                scale: self.scale.fetch(slot),
            }
        }
    }

    impl<'q> FetchItem<'q> for TransformQuery {
        type Item = Res<'q>;
    }

    impl<'w> Fetch<'w> for TransformQuery {
        const MUTABLE: bool = false;

        type Prepared = Prepared<'w>;

        fn prepare(&'w self, world: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared> {
            Some(Prepared {
                pos: self.pos.prepare(world, archetype)?,
                rot: self.rot.prepare(world, archetype)?,
                scale: self.scale.prepare(world, archetype)?,
            })
        }

        fn matches(&self, world: &World, archetype: &Archetype) -> bool {
            self.pos.matches(world, archetype)
                && self.rot.matches(world, archetype)
                && self.scale.matches(world, archetype)
        }

        fn describe(&self) -> String {
            todo!()
        }

        fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
            todo!()
        }

        fn difference(&self, archetype: &Archetype) -> Vec<String> {
            todo!()
        }
    }

    use flax::*;

    let mut world = World::new();

    let id1 = Entity::builder()
        .set(position(), vec3(3.4, 2.4, 2.1))
        .spawn(&mut world);

    let id2 = Entity::builder()
        .set(position(), vec3(7.4, 9.2, 3.4))
        .set(position(), vec3(3.4, 2.4, 2.1))
        .spawn(&mut world);

    // let mut query = Query::new(TransformQuery::as_fetch());
    // let query = query.iter(&world);
    // assert_eq!(query.get(id)
}
