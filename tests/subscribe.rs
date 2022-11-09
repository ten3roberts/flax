use flax::events::ArchetypeEvent;
use flax::*;

component! {
    a:f32,
    b:i32,
}

#[test]
#[cfg(feature = "flume")]
fn subscribe() {
    use flax::events::{ArchetypeSubscriber, SubscriberFilterExt};
    use flume::TryRecvError;
    use itertools::Itertools;

    let mut world = World::new();

    let id = Entity::builder()
        .set(a(), 1.5)
        .set(b(), 7)
        .spawn(&mut world);

    let (tx, rx) = flume::unbounded();

    world.subscribe(ArchetypeSubscriber::new(tx).filter(a().with()));

    assert_eq!(rx.try_recv(), Err(flume::TryRecvError::Empty));

    world.set(id, name(), "id".into()).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [ArchetypeEvent::Removed(id), ArchetypeEvent::Inserted(id)]
    );

    let id2 = Entity::builder()
        .set(a(), 5.7)
        .set(b(), 4)
        .spawn(&mut world);

    assert_eq!(rx.try_recv(), Ok(ArchetypeEvent::Inserted(id2)));

    let id3 = Entity::builder().set(b(), 7).spawn(&mut world);

    assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

    world.set(id3, a(), -4.1).unwrap();
    assert_eq!(rx.try_recv(), Ok(ArchetypeEvent::Inserted(id3)));

    world.remove(id, a()).unwrap();
    assert_eq!(rx.try_recv(), Ok(ArchetypeEvent::Removed(id)));
}

#[test]
#[cfg(feature = "flume")]
fn subscribe_inverted() {
    use flax::events::{ArchetypeSubscriber, SubscriberFilterExt};
    use flume::TryRecvError;

    let mut world = World::new();
    let (tx, rx) = flume::unbounded();
    world.subscribe(ArchetypeSubscriber::new(tx).filter(a().with() & b().without()));

    let id = Entity::builder()
        .set(a(), 1.5)
        .set(b(), 7)
        .spawn(&mut world);

    assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    world.remove(id, b()).unwrap();

    assert_eq!(rx.try_recv(), Ok(ArchetypeEvent::Inserted(id)));

    world.set(id, b(), 5).unwrap();
    assert_eq!(rx.try_recv(), Ok(ArchetypeEvent::Removed(id)));

    world.remove(id, b()).unwrap();
    assert_eq!(rx.try_recv(), Ok(ArchetypeEvent::Inserted(id)));

    world.remove(id, a()).unwrap();

    assert_eq!(rx.try_recv(), Ok(ArchetypeEvent::Removed(id)));
}

#[test]
#[cfg(feature = "flume")]
fn subscribe_filter() {
    use flax::events::{ChangeEvent, ChangeSubscriber, SubscriberFilterExt};
    use itertools::Itertools;

    let mut world = World::new();
    let (tx, rx) = flume::unbounded();
    world.subscribe(ChangeSubscriber::new(&[a().key()], tx).filter(b().with()));

    let id = Entity::builder()
        .set(a(), 1.5)
        .set(b(), 7)
        .spawn(&mut world);

    assert_eq!(
        rx.drain().collect_vec(),
        [ChangeEvent {
            kind: ChangeKind::Inserted,
            component: a().key()
        }]
    );

    world.set(id, a(), 7.0).unwrap();
    assert_eq!(
        rx.drain().collect_vec(),
        [ChangeEvent {
            kind: ChangeKind::Modified,
            component: a().key()
        }]
    );

    world.despawn(id).unwrap();

    assert_eq!(
        rx.drain().collect_vec(),
        [ChangeEvent {
            kind: ChangeKind::Removed,
            component: a().key()
        }]
    );
}
