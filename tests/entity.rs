use flax::{component, name, Entity, World};

component! {
    a: i32,
    b: String,

}

#[test]
#[cfg(feature = "flume")]
fn entity_ref() {
    use flax::{
        entity_ids,
        events::{
            ArchetypeSubscriber, ChangeEvent, ChangeSubscriber, ShapeEvent, ShapeSubscriber,
            SubscriberFilterExt,
        },
        Query,
    };
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();
    let (tx, changes) = flume::unbounded();
    world.subscribe(ChangeSubscriber::new(&[a().key(), b().key()], tx));
    let (tx, archetype_events) = flume::unbounded();
    world.subscribe(ShapeSubscriber::new(a().with(), tx));

    let mut query = Query::new(entity_ids()).filter(a().removed());

    let id = Entity::builder()
        .set(name(), "a".into())
        .set(a(), 5)
        .set(b(), "Foo".into())
        .spawn(&mut world);

    assert_eq!(
        archetype_events.drain().collect_vec(),
        [ShapeEvent::Matched(id)]
    );

    assert_eq!(query.borrow(&world).iter().collect_vec(), []);

    assert_eq!(
        changes.drain().collect_vec(),
        [
            ChangeEvent {
                kind: flax::ChangeKind::Inserted,
                component: a().key(),
            },
            ChangeEvent {
                kind: flax::ChangeKind::Inserted,
                component: b().key(),
            }
        ]
    );

    world.clear(id).unwrap();

    assert_eq!(
        archetype_events.drain().collect_vec(),
        [ShapeEvent::Unmatched(id)]
    );

    assert_eq!(
        changes.drain().collect_vec(),
        [
            ChangeEvent {
                kind: flax::ChangeKind::Removed,
                component: a().key(),
            },
            ChangeEvent {
                kind: flax::ChangeKind::Removed,
                component: b().key(),
            }
        ]
    );

    assert_eq!(query.borrow(&world).iter().collect_vec(), [id]);
}
