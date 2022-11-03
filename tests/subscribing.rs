use core::iter::once;
use core::iter::repeat;

use flax::events::ArchetypeSubscriber;
use flax::events::ChangeSubscriber;
use flax::events::SubscriberFilterExt;
use flax::{
    component, entity_ids,
    events::{ArchetypeEvent, ChangeEvent},
    name, Entity, Query, World,
};
use itertools::Itertools;
use pretty_assertions::assert_eq;

component! {
    a:i32,
    b:String,
    c:f32,
}

#[test]
#[cfg(feature = "flume")]
fn subscribing() {
    use flax::{
        entity_ids,
        events::{
            ArchetypeEvent, ArchetypeSubscriber, ChangeEvent, ChangeSubscriber, SubscriberFilterExt,
        },
        ChangeKind, Query,
    };
    use itertools::Itertools;

    let mut world = World::new();

    let (tx, events) = flume::unbounded();
    world.subscribe(ArchetypeSubscriber::new(tx).filter(a().with()));

    let (tx, changed) = flume::unbounded();
    world.subscribe(ChangeSubscriber::new(&[a().key()], tx));

    let mut q = Query::new(entity_ids()).filter(a().removed());

    q.borrow(&world);

    let id = Entity::builder()
        .set(a(), 5)
        .set(b(), "Foo".to_string())
        .spawn(&mut world);

    assert_eq!(
        changed.drain().collect_vec(),
        [ChangeEvent {
            kind: ChangeKind::Inserted,
            component: a().key(),
        }]
    );

    let id2 = Entity::builder().set(a(), 7).spawn(&mut world);

    world.remove(id, a()).unwrap();

    assert_eq!(
        changed.drain().collect_vec(),
        [
            ChangeEvent {
                kind: ChangeKind::Inserted,
                component: a().key(),
            },
            ChangeEvent {
                kind: ChangeKind::Removed,
                component: a().key(),
            }
        ]
    );

    *world.get_mut(id2, a()).unwrap() = 1;

    assert_eq!(
        changed.drain().collect_vec(),
        [ChangeEvent {
            kind: ChangeKind::Modified,
            component: a().key(),
        }]
    );

    world.set(id2, a(), 2).unwrap();

    assert_eq!(
        changed.drain().collect_vec(),
        [ChangeEvent {
            kind: ChangeKind::Modified,
            component: a().key(),
        }]
    );

    Query::new(a().as_mut())
        .borrow(&world)
        .iter()
        .for_each(|v| *v *= -1);

    assert_eq!(
        changed.drain().collect_vec(),
        [ChangeEvent {
            kind: ChangeKind::Modified,
            component: a().key(),
        }]
    );

    Query::new(b().as_mut())
        .borrow(&world)
        .iter()
        .for_each(|v| v.push('!'));

    assert_eq!(changed.drain().collect_vec(), []);

    assert_eq!(
        events.drain().collect_vec(),
        [
            ArchetypeEvent::Inserted(id),
            ArchetypeEvent::Inserted(id2),
            ArchetypeEvent::Removed(id)
        ]
    );

    world.set(id2, b(), "Bar".to_string()).unwrap();

    assert_eq!(q.borrow(&world).iter().collect_vec(), [id]);
}

#[tokio::test]
#[cfg(feature = "tokio")]
async fn tokio_subscribe() {
    use flax::events::ArchetypeSubscriber;
    use flax::events::SubscriberFilterExt;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio::sync::Notify;
    let notify = Arc::new(Notify::new());

    let mut world = World::new();

    let (tx, mut modified) = mpsc::unbounded_channel();
    world.subscribe(flax::events::ChangeSubscriber::new(&[a().key()], tx));

    world.subscribe(
        ArchetypeSubscriber::new(Arc::downgrade(&notify)).filter(a().with() | b().with()),
    );

    let id = Entity::builder().set(a(), 5).spawn(&mut world);

    notify.notified().await;

    assert_eq!(
        modified.recv().await,
        Some(ChangeEvent {
            kind: flax::ChangeKind::Inserted,
            component: a().key()
        })
    );

    world.remove(id, a()).unwrap();

    assert_eq!(
        modified.recv().await,
        Some(ChangeEvent {
            kind: flax::ChangeKind::Removed,
            component: a().key()
        })
    );
    notify.notified().await;
    world.set(id, b(), "Hello, World!".into()).unwrap();
    notify.notified().await;
}

#[test]
fn moving_changes() {
    let mut world = World::new();

    let (tx, tracking) = flume::unbounded();
    world.subscribe(ArchetypeSubscriber::new(tx).filter(a().with() & c().without()));
    let (tx, modified) = flume::unbounded();

    world.subscribe(ChangeSubscriber::new(&[a().key()], tx));

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
        tracking.drain().collect_vec(),
        ids.iter()
            .map(|&id| { ArchetypeEvent::Inserted(id) })
            .collect_vec()
    );

    assert_eq!(
        query.borrow(&world).iter().collect_vec(),
        ids.iter().copied().zip(repeat(&5)).collect_vec()
    );

    world.set(ids[3], a(), 7).unwrap();

    assert_eq!(
        modified.drain().collect_vec(),
        repeat(ChangeEvent {
            kind: flax::ChangeKind::Inserted,
            component: a().key()
        })
        .take(10)
        .chain(once(ChangeEvent {
            kind: flax::ChangeKind::Modified,
            component: a().key()
        }))
        .collect_vec()
    );

    for &id in &ids {
        world.set(id, b(), "Foo".into()).unwrap();
    }

    assert_eq!(tracking.drain().collect_vec(), []);
    world.set(ids[2], c(), 5.4).unwrap();
    world.set(ids[6], c(), 5.4).unwrap();
    world.set(ids[1], c(), 5.4).unwrap();

    assert_eq!(
        tracking.drain().collect_vec(),
        [ids[2], ids[6], ids[1]]
            .iter()
            .map(|&id| { ArchetypeEvent::Removed(id) })
            .collect_vec()
    );

    assert_eq!(modified.drain().collect_vec(), []);

    // Make sure the change survived the migrations
    assert_eq!(query.borrow(&world).iter().collect_vec(), [(ids[3], &7)]);
}
