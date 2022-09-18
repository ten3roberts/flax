use std::iter::repeat;

use flax::*;

#[test]
fn add_remove() {
    component! {
        a: f32,
        b: f32,
    }
    let mut world = World::default();
    let mut batch = BatchSpawn::new(1000);

    batch.set(a(), repeat(0.0)).unwrap();
    let ids = batch.spawn(&mut world);
    // for _ in 0..1000 {
    for id in &ids {
        world.set(*id, b(), 0.0).unwrap();
    }

    let mut q = Query::new(b());
    assert_eq!(q.borrow(&world).count(), 1000);

    for id in &ids {
        world.remove(*id, b()).unwrap();
    }

    assert_eq!(q.borrow(&world).count(), 0);
    assert_eq!(Query::new(a()).borrow(&world).count(), 1000);
}
