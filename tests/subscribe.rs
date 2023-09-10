use flax::*;

component! {
    a:f32,
    b:i32,
}

#[test]
#[cfg(feature = "flume")]
fn subscribe() {
    use flax::events::{Event, EventKind, EventSubscriber};
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();

    let id = Entity::builder()
        .set(a(), 1.5)
        .set(b(), 7)
        .spawn(&mut world);

    let (tx, rx) = flume::unbounded::<Event>();

    world.subscribe(tx.filter_arch(a().with()));

    assert_eq!(rx.try_recv(), Err(flume::TryRecvError::Empty));

    world.set(id, name(), "id".into()).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id,
            key: name().key(),
            kind: EventKind::Added
        }],
    );

    let id2 = Entity::builder()
        .set(a(), 5.7)
        .set(b(), 4)
        .spawn(&mut world);

    assert_eq!(
        rx.drain().collect_vec(),
        [
            Event {
                id: id2,
                key: a().key(),
                kind: EventKind::Added
            },
            Event {
                id: id2,
                key: b().key(),
                kind: EventKind::Added
            }
        ]
    );

    let id3 = Entity::builder().set(b(), 7).spawn(&mut world);

    assert_eq!(rx.drain().collect_vec(), []);

    world.set(id3, a(), -4.1).unwrap();
    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id: id3,
            key: a().key(),
            kind: EventKind::Added
        }]
    );

    *world.get_mut(id3, a()).unwrap() = 4.0;

    world.remove(id, a()).unwrap();
    assert_eq!(
        rx.drain().collect_vec(),
        [
            Event {
                id: id3,
                key: a().key(),
                kind: EventKind::Modified
            },
            Event {
                id,
                key: a().key(),
                kind: EventKind::Removed
            }
        ]
    );
}

#[test]
#[cfg(feature = "flume")]
fn subscribe_inverted() {
    use flax::events::{Event, EventKind, EventSubscriber};
    use flume::TryRecvError;
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();
    let (tx, rx) = flume::unbounded();
    world.subscribe(tx.filter_arch((a().with(), b().without())));

    let id = Entity::builder()
        .set(a(), 1.5)
        .set(b(), 7)
        .spawn(&mut world);

    assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    world.remove(id, b()).unwrap();
    world.set(id, name(), "id".into()).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id,
            key: name().key(),
            kind: EventKind::Added
        }]
    );

    world.set(id, b(), 5).unwrap();

    // Not detected since the event is generated *from* the archetype containing `b`
    assert_eq!(rx.drain().collect_vec(), []);

    // `id` is now in a blocked archetype
    world.set(id, name(), "id".into()).unwrap();

    assert_eq!(rx.drain().collect_vec(), []);

    world.remove(id, b()).unwrap();

    world.remove(id, a()).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id,
            key: a().key(),
            kind: EventKind::Removed
        }]
    );

    assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    drop(world);
    assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
}

#[test]
#[cfg(feature = "flume")]
fn subscribe_filter() {
    use flax::events::{Event, EventKind, EventSubscriber};
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();
    let (tx, rx) = flume::unbounded();
    world.subscribe(
        tx.filter_components([a().key(), b().key()])
            .filter_arch(b().with()),
    );

    let id = Entity::builder()
        .set(a(), 1.5)
        .set(b(), 7)
        .spawn(&mut world);

    assert_eq!(
        rx.drain().collect_vec(),
        [
            Event {
                id,
                key: a().key(),
                kind: EventKind::Added,
            },
            Event {
                id,
                key: b().key(),
                kind: EventKind::Added,
            }
        ]
    );

    world.set(id, a(), 7.0).unwrap();
    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id,
            key: a().key(),
            kind: EventKind::Modified,
        }]
    );

    // Events are not generated if `b` is removed
    // The event for removing b is still generated since the event is generated before the
    // entity is moved to another archetype
    world.remove(id, b()).unwrap();

    world.set(id, a(), 7.0).unwrap();
    assert_eq!(
        rx.drain().collect_vec(),
        [Event {
            id,
            key: b().key(),
            kind: EventKind::Removed
        }]
    );

    world.set(id, b(), 0).unwrap();

    world.despawn(id).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [
            Event {
                id,
                key: b().key(),
                kind: EventKind::Added,
            },
            Event {
                id,
                key: a().key(),
                kind: EventKind::Removed,
            },
            Event {
                id,
                key: b().key(),
                kind: EventKind::Removed,
            }
        ]
    );
}
