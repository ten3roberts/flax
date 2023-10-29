use flax::component;

component! {
    a:i32,
    b:String,
    c:f32,
}

#[test]
#[cfg(feature = "flume")]
fn subscribing() {
    use flax::{
        events::{Event, EventKind, EventSubscriber},
        Entity, Query, World,
    };
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();

    let (tx, rx) = flume::unbounded();
    world.subscribe(tx.filter_components([a().key()]));

    let id = Entity::builder()
        .set(a(), 5)
        .set(b(), "Foo".to_string())
        .spawn(&mut world);

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id,
            key: a().key(),
            kind: flax::events::EventKind::Added,
        }]
    );

    let id2 = Entity::builder().set(a(), 7).spawn(&mut world);

    world.remove(id, a()).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [
            Event {
                id: id2,
                kind: EventKind::Added,
                key: a().key(),
            },
            Event {
                id,
                kind: EventKind::Removed,
                key: a().key(),
            },
        ]
    );

    *world.get_mut(id2, a()).unwrap() = 1;

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id: id2,
            kind: EventKind::Modified,
            key: a().key(),
        }]
    );

    world.set(id2, a(), 2).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id: id2,
            kind: EventKind::Modified,
            key: a().key(),
        }]
    );

    Query::new(a().as_mut())
        .borrow(&world)
        .iter()
        .for_each(|v| *v *= -1);

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id: id2,
            kind: EventKind::Modified,
            key: a().key(),
        }]
    );

    Query::new(b().as_mut())
        .borrow(&world)
        .iter()
        .for_each(|v| v.push('!'));

    assert_eq!(rx.drain().collect_vec(), []);

    world.set(id2, b(), "Bar".to_string()).unwrap();
}

#[test]
#[cfg(feature = "flume")]
fn subscribing_with_value() {
    use flax::{
        events::{Event, EventKind, WithValue},
        Entity, Query, World,
    };
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();

    let (tx, rx) = flume::unbounded::<(Event, i32)>();

    world.subscribe(WithValue::new(a(), tx));

    let id = Entity::builder()
        .set(a(), 5)
        .set(b(), "Foo".to_string())
        .spawn(&mut world);

    assert_eq!(
        rx.drain().collect_vec(),
        [(
            Event {
                id,
                key: a().key(),
                kind: flax::events::EventKind::Added,
            },
            5
        )]
    );

    let id2 = Entity::builder().set(a(), 7).spawn(&mut world);
    world.set(id2, a(), 3).unwrap();

    world.remove(id, a()).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [
            (
                Event {
                    id: id2,
                    kind: EventKind::Added,
                    key: a().key(),
                },
                7
            ),
            (
                Event {
                    id,
                    kind: EventKind::Removed,
                    key: a().key(),
                },
                5
            )
        ]
    );

    *world.get_mut(id2, a()).unwrap() = 1;

    world.set(id2, a(), 2).unwrap();

    Query::new(a().as_mut())
        .borrow(&world)
        .iter()
        .for_each(|v| *v *= -1);

    Query::new(b().as_mut())
        .borrow(&world)
        .iter()
        .for_each(|v| v.push('!'));

    assert_eq!(world.remove(id2, a()).unwrap(), -2);

    assert_eq!(
        rx.drain().collect_vec(),
        [(
            Event {
                id: id2,
                kind: EventKind::Removed,
                key: a().key()
            },
            -2
        )]
    );

    world.set(id2, b(), "Bar".to_string()).unwrap();
}

#[tokio::test]
#[cfg(feature = "tokio")]
async fn tokio_subscribe() {
    use flax::events::Event;
    use flax::events::EventKind;
    use flax::events::EventSubscriber;
    use flax::*;
    use futures::FutureExt;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio::sync::Notify;
    let notify = Arc::new(Notify::new());

    let mut world = World::new();

    let (tx, mut rx) = mpsc::unbounded_channel();
    world.subscribe(tx.filter_components([a().key()]));

    world.subscribe(Arc::downgrade(&notify).filter_arch(filter::Or((a().with(), b().with()))));

    let id = Entity::builder().set(a(), 5).spawn(&mut world);

    notify.notified().now_or_never().unwrap();

    assert_eq!(
        rx.recv().now_or_never().unwrap(),
        Some(Event {
            id,
            key: a().key(),
            kind: EventKind::Added,
        })
    );

    world.remove(id, a()).unwrap();

    assert_eq!(
        rx.recv().now_or_never().unwrap(),
        Some(Event {
            id,
            key: a().key(),
            kind: EventKind::Removed,
        })
    );

    notify.notified().now_or_never().unwrap();
    world.set(id, b(), "Hello, World!".into()).unwrap();
    notify.notified().now_or_never().unwrap();
}

#[test]
#[cfg(feature = "flume")]
fn moving_changes() {
    use std::iter::repeat;

    use flax::{
        components::name,
        events::{Event, EventKind, EventSubscriber},
        *,
    };
    use itertools::Itertools;

    let mut world = World::new();

    // world.subscribe(ShapeSubscriber::new(a().with() & c().without(), tx));
    let (tx, rx) = flume::unbounded();

    // world.subscribe(ChangeSubscriber::new(&[a().key()], tx));

    world.subscribe(tx.filter_components([a().key(), c().key()]));

    let ids = (0..10)
        .map(|i| {
            Entity::builder()
                .set(name(), i.to_string())
                .set(a(), 5)
                .spawn(&mut world)
        })
        .collect_vec();

    let mut query = Query::new((entity_ids(), a().modified()));

    assert_eq!(
        rx.drain().collect_vec(),
        ids.iter()
            .map(|&id| {
                Event {
                    id,
                    key: a().key(),
                    kind: EventKind::Added,
                }
            })
            .collect_vec()
    );

    assert_eq!(
        query.borrow(&world).iter().collect_vec(),
        ids.iter().copied().zip(repeat(&5)).collect_vec()
    );

    world.set(ids[3], a(), 7).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id: ids[3],
            key: a().key(),
            kind: EventKind::Modified,
        }]
    );

    for &id in &ids {
        world.set(id, b(), "Foo".into()).unwrap();
    }

    world.set(ids[2], c(), 5.4).unwrap();
    world.set(ids[6], c(), 5.4).unwrap();
    world.set(ids[1], c(), 5.4).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [ids[2], ids[6], ids[1]]
            .iter()
            .map(|&id| Event {
                id,
                key: c().key(),
                kind: EventKind::Added
            })
            .collect_vec()
    );

    // Make sure the change survived the migrations
    assert_eq!(query.borrow(&world).iter().collect_vec(), [(ids[3], &7)]);
}
