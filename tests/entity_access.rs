use flax::{component, name, Entity, FetchExt, World};

#[test]
fn entity_access() {
    component! {
        a: i32,
        b: String,
    }

    let mut world = World::new();

    let id = Entity::builder()
        .set(name(), "a".into())
        .set(a(), 5)
        .set(b(), "Foo".into())
        .spawn(&mut world);

    let entity = world.entity(id).unwrap();

    let query = &(name().cloned(), a());
    let query2 = &(name().cloned(), a().as_mut());
    {
        let mut query = entity.query_one(query);
        assert_eq!(query.get(), Some(("a".into(), &5)));
    }

    {
        let mut query = entity.query_one(query2);
        *query.get().unwrap().1 += 1;

        assert_eq!(query.get(), Some(("a".into(), &mut 6)));
    }

    let mut query = entity.query_one(query);
    assert_eq!(query.get(), Some(("a".into(), &6)));
}
