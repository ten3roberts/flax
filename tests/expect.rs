use flax::{components::name, Entity, FetchExt, Query, World};

#[test]
#[should_panic(expected = "Expected name to be present on entity")]
fn expect_present() {
    let mut world = World::new();

    Entity::builder().spawn(&mut world);
    let mut query = Query::new(name().cloned().expect());

    let _ = query.collect_vec(&world);
}
