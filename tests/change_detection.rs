use flax::*;
use glam::{Quat, Vec3};
use itertools::Itertools;
use rand::{rngs::StdRng, Rng, SeedableRng};

#[test]
fn change_detection() {
    component! {
        position: Vec3,
        rotation: Quat,
    }

    let mut world = World::new();

    let mut rng = StdRng::seed_from_u64(83);
    let mut ids = (0..10)
        .map(|i| {
            Entity::builder()
                .set(name(), format!("a.{i}"))
                .set(position(), rng.gen())
                .spawn(&mut world)
        })
        .collect_vec();

    ids.extend((0..30).map(|i| {
        Entity::builder()
            .set(name(), format!("b.{i}"))
            .set(position(), rng.gen())
            .set(rotation(), rng.gen())
            .spawn(&mut world)
    }));

    let mut query = Query::new((entities(), position().modified()));

    assert_eq!(
        query
            .borrow(&world)
            .iter()
            .map(|v| v.0)
            .sorted()
            .collect_vec(),
        ids
    );

    for &id in &ids[20..40] {
        world.set(id, position(), Vec3::ZERO).unwrap();
    }

    assert_eq!(
        query
            .borrow(&world)
            .iter()
            .map(|v| v.0)
            .sorted()
            .collect_vec(),
        ids[20..40]
    );

    let mut query = Query::new((entities(), position(), rotation().removed()));

    assert_eq!(
        query
            .borrow(&world)
            .iter()
            .map(|v| v.0)
            .sorted()
            .collect_vec(),
        vec![]
    );

    world.remove(ids[11], rotation()).unwrap();
    world.remove(ids[12], rotation()).unwrap();
    world.remove(ids[30], rotation()).unwrap();

    assert_eq!(
        query
            .borrow(&world)
            .iter()
            .map(|v| v.0)
            .sorted()
            .collect_vec(),
        vec![ids[11], ids[12], ids[30]]
    );
}
