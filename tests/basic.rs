use flax::World;

#[test]
fn creation() {
    let mut world = World::new();

    let a = world.spawn();

    assert!(world.is_alive(a));
    world.despawn(a);
    assert!(!world.is_alive(a));
}
