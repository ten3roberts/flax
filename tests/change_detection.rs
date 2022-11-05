use flax::*;
use glam::{Quat, Vec3};
use itertools::Itertools;
use rand::{rngs::StdRng, Rng, SeedableRng};

#[test]
#[cfg(feature = "flume")]
fn change_detection() {
    let (removed_tx, removed_rx) = flume::unbounded();

    component! {
        position: Vec3,
        rotation: Quat,
    }

    let mut world = World::new();

    world.on_removed(rotation(), removed_tx);

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

    let mut query = Query::new((entity_ids(), position().modified()));

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

    let mut query = Query::new((entity_ids(), position(), rotation().removed()));

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

    let removed = removed_rx
        .drain()
        .inspect(|v| eprintln!("removed: {v:?}"))
        .map(|v| v.0)
        .collect_vec();

    assert_eq!(removed, [ids[11], ids[12], ids[30]]);

    world.despawn(ids[35]).unwrap();

    let removed = removed_rx.drain().map(|v| v.0).collect_vec();
    assert_eq!(removed, [ids[35]]);

    dbg!(removed);
}
