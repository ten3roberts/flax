use flax::{
    child_of, component,
    events::{ChangeEvent, ChangeSubscriber, ShapeSubscriber},
    name, Entity, Query, RelationExt, World,
};
use itertools::Itertools;

component! {
    a: i32,
    b: String,

}

#[test]
#[cfg(feature = "flume")]
fn entity_ref() {
    use flax::{
        entity_ids,
        events::{ChangeEvent, ChangeSubscriber, ShapeEvent, ShapeSubscriber},
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

#[test]
#[cfg(feature = "flume")]
fn entity_hierarchy() {
    use pretty_assertions::assert_eq;

    let mut world = World::new();
    let (tx, rx) = flume::unbounded();

    world.subscribe(ShapeSubscriber::new(
        name().with() & child_of.with_relation(),
        tx,
    ));

    let (tx, track_a) = flume::unbounded();
    world.subscribe(ChangeSubscriber::new(&[a().key()], tx));

    let id = Entity::builder()
        .set(name(), "root".into())
        .set(a(), 1)
        .attach(child_of, Entity::builder().set(name(), "child_1".into()))
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "child_2".into())
                .attach(child_of, Entity::builder().set(name(), "child_2_1".into())),
        )
        .spawn(&mut world);

    assert_eq!(rx.drain().len(), 3);
    assert_eq!(
        track_a.drain().collect_vec(),
        [ChangeEvent {
            kind: flax::ChangeKind::Inserted,
            component: a().key()
        }]
    );
    assert_eq!(
        Query::new(name())
            .borrow(&world)
            .iter()
            .cloned()
            .sorted()
            .collect_vec(),
        [
            "child_1".to_string(),
            "child_2".into(),
            "child_2_1".into(),
            "root".into()
        ]
    );

    world.despawn_children(id, child_of).unwrap();

    assert_eq!(rx.drain().len(), 3);
    assert_eq!(
        Query::new(name())
            .borrow(&world)
            .iter()
            .cloned()
            .collect_vec(),
        ["root".to_string()]
    );

    let mut entity = world.entity_mut(id).unwrap();
    assert_eq!(entity.get(name()).as_deref(), Ok(&"root".to_string()));
    assert_eq!(entity.get(a()).as_deref(), Ok(&1));

    entity.retain(|k| k == name().key());

    assert_eq!(entity.get(name()).as_deref(), Ok(&"root".to_string()));
    assert_eq!(
        entity.get(a()).as_deref(),
        Err(&flax::Error::MissingComponent(id, a().info()))
    );
    assert_eq!(rx.drain().collect_vec(), []);
    assert_eq!(
        track_a.drain().collect_vec(),
        [ChangeEvent {
            kind: flax::ChangeKind::Removed,
            component: a().key()
        }]
    );
}
