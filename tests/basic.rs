use flax::{component, World};

#[test]
fn creation() {
    component! {
        a: i32,
        b: String,
    }

    let mut world = World::new();

    let e = world.spawn();
    world.insert(e, a(), 5);

    assert!(world.is_alive(e));
    world.despawn(e);
    assert!(!world.is_alive(e));
}
