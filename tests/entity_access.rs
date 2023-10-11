use flax::{component, name, Entity, World};

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
}
