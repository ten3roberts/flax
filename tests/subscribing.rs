use flax::{component, Entity, World};

#[test]
#[cfg(feature = "flume")]
fn subscribing() {
    use flax::{
        entity_ids,
        events::{ArchetypeEvent, ChangeEvent},
        ChangeKind, Query,
    };
    use itertools::Itertools;

    component! {
        a:i32,
        b:String,
        c:f32,
    }

    let mut world = World::new();

    let (tx, events) = flume::unbounded();
    world.subscribe(a().with(), tx);

    let (tx, changed) = flume::unbounded();
    world.subscribe_changed(a().with(), &[a().key()], tx);

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
