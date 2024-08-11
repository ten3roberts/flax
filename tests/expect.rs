use flax::{components::name, Entity, FetchExt, Query, World};

#[test]
#[should_panic(expected = "name must be present")]
fn expect_present() {
    let mut world = World::new();

    Entity::builder().spawn(&mut world);
    let mut query = Query::new(name().cloned().expect("name must be present"));

    let _ = query.collect_vec(&world);
}
