use flax::*;

component! {
    a: i32,
    b: String,
}

#[test]
#[cfg(feature = "flume")]
fn entity_ref() {
    use flax::{
        components::name,
        events::{Event, EventKind, EventSubscriber},
    };
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();
    let (tx, changes) = flume::unbounded();
    world.subscribe(tx.filter_components([a().key(), b().key()]));

    // let mut query = Query::new(entity_ids()).filter(a().removed());

    let id = Entity::builder()
        .set(name(), "a".into())
        .set(a(), 5)
        .set(b(), "Foo".into())
        .spawn(&mut world);

    assert_eq!(
        changes.drain().collect_vec(),
        [
            Event {
                id,
                key: a().key(),
                kind: EventKind::Added
            },
            Event {
                id,
                key: b().key(),
                kind: EventKind::Added
            }
        ]
    );

    // assert_eq!(query.borrow(&world).iter().collect_vec(), []);

    world.clear(id).unwrap();

    assert_eq!(
        changes.drain().collect_vec(),
        [
            Event {
                id,
                key: a().key(),
                kind: EventKind::Removed
            },
            Event {
                id,
                key: b().key(),
                kind: EventKind::Removed
            }
        ]
    );

    // assert_eq!(query.borrow(&world).iter().collect_vec(), [id]);
}

#[test]
#[cfg(feature = "flume")]
fn entity_hierarchy() {
    use flax::{
        components::{child_of, name},
        error::MissingComponent,
        events::{Event, EventSubscriber},
    };
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    let mut world = World::new();
    let (tx, rx) = flume::unbounded::<Event>();

    world.subscribe(
        tx.filter_arch((name().with(), child_of.with_relation()))
            .filter_components([name().key()]),
    );

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

    let mut query = Query::new(name().cloned());
    assert_eq!(
        query.collect_sorted_vec(&world),
        ["child_1", "child_2", "child_2_1", "root"]
    );

    world.despawn_children(id, child_of).unwrap();
    assert_eq!(query.collect_sorted_vec(&world), ["root"]);

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
        Err(&MissingComponent {
            id,
            desc: a().desc()
        })
    );

    assert_eq!(rx.drain().collect_vec(), []);
}
