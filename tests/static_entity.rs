use flax::*;

component! {
    resources,
    value: String,
}

#[test]
fn static_entity_set() {
    let mut world = World::new();

    let mut query = Query::new(value());

    assert!(query.borrow(&world).get(resources()).is_err());

    world.set(resources(), value(), "FooBar".into()).unwrap();

    assert_eq!(query.borrow(&world).get(resources()).unwrap(), "FooBar");

    assert_eq!(
        world.entity(resources()).unwrap().get(value()).as_deref(),
        Ok(&"FooBar".into())
    );
}

#[test]
fn query() {
    let mut world = World::new();

    let mut query = Query::new(entity_ids());

    assert!(query.borrow(&world).get(resources()).is_err());

    world
        .entity_mut(resources())
        .unwrap()
        .set(value(), "Baz".into())
        .unwrap();

    assert!(query.borrow(&world).get(resources()).is_ok());
}
