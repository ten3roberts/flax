use flax::{components::name, *};
use itertools::Itertools;
use pretty_assertions::assert_eq;

#[test]
fn no_changes() {
    let mut world = World::new();

    component! {
        a: i32,
        b: i32,
    }

    let mut query = Query::new(entity_ids()).filter(a().modified() | b().modified());

    let id = Entity::builder().set(a(), 5).spawn(&mut world);

    assert_eq!(query.collect_vec(&world), [id]);
}

#[test]
#[cfg(feature = "flume")]
fn change_detection() {
    use flax::{components::name, *};
    use glam::{Quat, Vec3};
    use itertools::Itertools;
    use pretty_assertions::assert_eq;
    use rand::{rngs::StdRng, Rng, SeedableRng};

    use flax::events::{EventKind, EventSubscriber};

    let (removed_tx, removed_rx) = flume::unbounded();

    component! {
        position: Vec3,
        rotation: Quat,
    }

    let mut world = World::new();

    world.subscribe(
        removed_tx
            .filter_components([rotation().key()])
            .filter(|v| v.kind == EventKind::Removed),
    );

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
            .set(rotation(), Quat::from_scaled_axis(rng.gen()))
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

    {
        let mut borrow = query.borrow(&world);
        assert_eq!(
            borrow.iter().map(|v| v.0).sorted().collect_vec(),
            vec![ids[11], ids[12], ids[30]]
        );
    }
    let removed = removed_rx
        .drain()
        .inspect(|v| eprintln!("removed: {v:?}"))
        .map(|v| v.id)
        .collect_vec();

    assert_eq!(removed, [ids[11], ids[12], ids[30]]);

    world.despawn(ids[35]).unwrap();

    let removed = removed_rx.drain().map(|v| v.id).collect_vec();
    assert_eq!(removed, [ids[35]]);

    dbg!(removed);
}

#[test]
#[cfg(feature = "flume")]
fn clear_events() {
    use flax::{
        components::{component_info, name},
        events::{Event, EventSubscriber},
    };

    let mut world = World::new();

    let (tx, rx) = flume::unbounded();
    world.subscribe(
        tx.filter_components([name().key()])
            .filter_arch(component_info().without()),
    );

    let mut query = Query::new(entity_ids()).filter(name().removed());

    let id = Entity::builder().set(name(), "id".into()).spawn(&mut world);

    assert_eq!(query.collect_vec(&world), &[]);

    world.clear(id).unwrap();

    assert_eq!(query.collect_vec(&world), &[id]);

    assert_eq!(
        rx.drain().collect_vec(),
        &[
            Event {
                id,
                key: name().key(),
                kind: events::EventKind::Added,
            },
            Event {
                id,
                key: name().key(),
                kind: events::EventKind::Removed,
            }
        ]
    );
}

#[test]
#[cfg(feature = "flume")]
fn despawn() {
    use flax::{
        components::{component_info, name},
        events::{Event, EventSubscriber},
    };

    let mut world = World::new();

    let (tx, rx) = flume::unbounded();
    world.subscribe(
        tx.filter_components([name().key()])
            .filter_arch(component_info().without()),
    );

    let id = Entity::builder().set(name(), "id".into()).spawn(&mut world);

    world.clear(id).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        &[
            Event {
                id,
                key: name().key(),
                kind: events::EventKind::Added,
            },
            Event {
                id,
                key: name().key(),
                kind: events::EventKind::Removed,
            }
        ]
    );
}
#[test]
fn query_changes() {
    component! {
        a: i32,
        b: i32,
        c: f32,
    };

    let mut world = World::new();

    let ids = (0..10)
        .map(|i| Entity::builder().set(a(), i).into())
        .chain((0..30).map(|i| Entity::builder().set(a(), i).set(b(), -i).into()))
        .chain((0..80).map(|i| {
            Entity::builder()
                .set(a(), i)
                .set(b(), -i)
                .set(c(), i as f32)
                .into()
        }))
        .enumerate()
        .map(|(i, mut v): (usize, EntityBuilder)| v.set(name(), format!("{i}")).spawn(&mut world))
        .collect_vec();

    let mut changed = Query::new((entity_ids(), a().modified().copied()));

    let mut changed = |w| changed.borrow(w).iter().map(|v| v.0).collect_vec();

    assert_eq!(changed(&world), ids);

    Query::new(a().as_mut())
        .with(b())
        .without(c())
        .borrow(&world)
        .for_each(|v| *v *= 2);

    assert_eq!(changed(&world), &ids[10..40]);

    Query::new(a().as_mut())
        .with(b())
        .filter(!c().with() | c().gt(40.0))
        .borrow(&world)
        .for_each(|v| *v *= 2);

    assert_eq!(
        changed(&world),
        [&ids[10..40], &ids[(40 + 41)..(40 + 80)]].concat()
    );
}
