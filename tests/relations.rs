use flax::{
    components::{child_of, name},
    events::{Event, EventSubscriber},
    fetch::relations_like_mut,
    filter::All,
    relation::RelationExt,
    *,
};
use itertools::Itertools;

#[test]
fn relations() {
    let mut world = World::new();

    let parent = Entity::builder()
        .set(name(), "Parent".into())
        .spawn(&mut world);

    let child1 = Entity::builder()
        .set(name(), "Child1".into())
        .set_default(child_of(parent))
        .spawn(&mut world);

    let child2 = Entity::builder()
        .set(name(), "Child2".into())
        .set_default(child_of(parent))
        .spawn(&mut world);

    let parent2 = Entity::builder()
        .set(name(), "Parent2".into())
        .spawn(&mut world);

    world.set(child1, child_of(parent2), ()).unwrap();

    assert_eq!(world.get(child1, child_of(parent2)).as_deref(), Ok(&()));

    let children = Query::new(entity_ids())
        .with(child_of(parent))
        .borrow(&world)
        .iter()
        .sorted()
        .collect_vec();

    assert_eq!(children, [child2]);
    tracing::info!("Children: {children:?}");

    let parents = Query::new(entity_ids())
        .filter(child_of.without_relation())
        .borrow(&world)
        .iter()
        .collect_vec();

    assert_eq!(parents, [parent, parent2]);
    assert!(world.has(child1, child_of(parent2)));

    assert!(!world.has(child1, child_of(parent)));
    world.despawn(parent2).unwrap();

    assert!(!world.has(child1, child_of(parent2)));

    world.despawn_recursive(parent, child_of).unwrap();

    // Moved and detached
    assert!(world.is_alive(child1));
    assert!(!world.is_alive(child2));

    tracing::info!("World: {world:#?}");

    world.despawn_many(All);

    assert_eq!(
        Query::new(()).borrow(&world).count(),
        0,
        "World was not empty"
    );

    let root = EntityBuilder::new()
        .set(name(), "parent".into())
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "child1".into())
                .attach(child_of, Entity::builder().set(name(), "child1.1".into())),
        )
        .attach(child_of, Entity::builder().set(name(), "child2".into()))
        .spawn(&mut world);

    assert_eq!(Query::new(child_of(root)).borrow(&world).count(), 2);
    assert_eq!(
        Query::new(())
            .filter(child_of.with_relation())
            .batch_size(1)
            .borrow(&world)
            .count(),
        3
    );
}

#[test]
fn multiple_hierarchies() {
    let mut world = World::new();

    component! {
        a(parent): (),
        b(parent): String,
    }

    let root = Entity::builder()
        .set(name(), "root".into())
        .attach(
            a,
            Entity::builder()
                .set(name(), "root.child(a)".into())
                .attach(
                    b,
                    Entity::builder().set(name(), "root.child(a).child(b)".into()),
                )
                .attach(
                    a,
                    Entity::builder().set(name(), "root.child(a).child(a)".into()),
                ),
        )
        .attach_with(
            b,
            "RelationValue".into(),
            Entity::builder().set(name(), "root.child(b)".into()),
        )
        .spawn(&mut world);

    eprintln!("World: {world:#?}");

    let children = Query::new(name().cloned())
        .with_relation(a)
        .collect_vec(&world);

    assert_eq!(children.len(), 2, "{children:#?}");

    let children = Query::new(entity_ids()).with(a(root)).collect_vec(&world);

    {
        assert_eq!(children.len(), 1, "{children:#?}");
        let child = world.entity_mut(children[0]).unwrap();

        let relations = child.relations(a).collect_vec();
        assert_eq!(relations.len(), 1);

        assert_eq!(relations[0].0, root);
    }

    let children = Query::new(entity_ids()).with(b(root)).collect_vec(&world);

    assert_eq!(children.len(), 1, "{children:#?}");
    let child = world.entity(children[0]).unwrap();

    let relations = child.relations(b).collect_vec();
    assert_eq!(relations.len(), 1);

    assert_eq!(relations[0].0, root);
    assert_eq!(&*relations[0].1, "RelationValue");
}

#[test]
fn many_detach() {
    component! {
        child_of(id): &'static str,
    }

    let mut world = World::new();

    let parent = Entity::builder()
        .set(name(), "Parent".into())
        .spawn(&mut world);

    let child1 = Entity::builder()
        .set(name(), "Child1".into())
        .set(child_of(parent), "first")
        .spawn(&mut world);

    let child2 = Entity::builder()
        .set(name(), "Child2".into())
        .set(child_of(parent), "first")
        .spawn(&mut world);

    // ANCHOR_END: relation_basic

    // ANCHOR: many_to_many
    let parent2 = Entity::builder()
        .set(name(), "Parent2".into())
        .spawn(&mut world);

    world.set(child1, child_of(parent2), "second").unwrap();

    tracing::info!("World: {world:#?}");

    world.set(child1, child_of(child2), "second").unwrap();
    world.set(child1, child_of(parent2), "third").unwrap();

    assert_eq!(
        Query::new(relations_like(child_of))
            .borrow(&world)
            .get(child1)
            .unwrap()
            .collect_vec(),
        [(parent, &"first"), (child2, &"second"), (parent2, &"third")]
    );

    assert_eq!(
        Query::new((
            entity_ids(),
            child_of.first_relation(),
            child_of.nth_relation(2).opt(),
        ))
        .borrow(&world)
        .iter()
        .sorted()
        .collect_vec(),
        [
            (child1, (parent, &"first"), Some((parent2, &"third"))),
            (child2, (parent, &"first"), None)
        ]
    );
    // ANCHOR_END: many_to_many
    // ANCHOR: query

    // Mathes a relation exactly
    let children_of_parent: Vec<Entity> = Query::new(entity_ids())
        .with(child_of(parent))
        .borrow(&world)
        .iter()
        .sorted()
        .collect_vec();

    assert_eq!(children_of_parent, [child1, child2]);

    let children_of_parent2: Vec<Entity> = Query::new(entity_ids())
        .with(child_of(parent2))
        .borrow(&world)
        .iter()
        .sorted()
        .collect_vec();

    assert_eq!(children_of_parent2, [child1]);

    // Matches a relation with any parent
    let all_children: Vec<Entity> = Query::new(entity_ids())
        .filter(child_of.with_relation())
        .borrow(&world)
        .iter()
        .sorted()
        .collect_vec();

    assert_eq!(all_children, [child1, child2]);

    let roots = Query::new(entity_ids())
        .filter(child_of.without_relation())
        .borrow(&world)
        .iter()
        .sorted()
        .collect_vec();

    assert_eq!(roots, [parent, parent2]);

    // ANCHOR_END: query

    // ANCHOR: lifetime

    assert!(world.has(child1, child_of(parent2)));

    world.despawn(parent2).unwrap();

    assert!(!world.has(child1, child_of(parent2)));

    world.despawn_recursive(parent, child_of).unwrap();
}

#[test]
fn despawn_recursive() {
    component! {
        child_of(id): &'static str,
    }

    let mut world = World::new();

    let parent = Entity::builder()
        .set(name(), "parent".into())
        .spawn(&mut world);

    let child1 = Entity::builder()
        .set(name(), "child1".into())
        .set(child_of(parent), "first")
        .spawn(&mut world);

    let _child2 = Entity::builder()
        .set(name(), "child2".into())
        .set(child_of(child1), "first")
        .spawn(&mut world);

    let mut query = Query::new(name()).filter(parent.traverse(child_of));

    assert_eq!(
        query.borrow(&world).iter().sorted().collect_vec(),
        &["child1", "child2", "parent",]
    );

    world.despawn_recursive(parent, child_of).unwrap();

    assert!(query
        .borrow(&world)
        .iter()
        .sorted()
        .collect_vec()
        .is_empty());
}

#[test]
fn relation_target_search() {
    component! {
        target(id1): (),
    }

    let mut world = World::new();
    let id1 = Entity::builder().spawn(&mut world);
    let id2 = Entity::builder().set(target(id1), ()).spawn(&mut world);
    let id3 = Entity::builder().set(child_of(id2), ()).spawn(&mut world);
    let id4 = Entity::builder().set(child_of(id3), ()).spawn(&mut world);

    let query = target.first_relation().traverse(child_of);
    let entity = world.entity(id4).unwrap();
    let mut query = entity.query(&query);
    assert_eq!(query.get(), Some((id1, &())));
}

#[test]
fn exclusive() {
    component! {
        child_of(parent): () => [ Exclusive ],
    }

    let mut world = World::new();

    let id1 = Entity::builder().spawn(&mut world);
    let id2 = Entity::builder().spawn(&mut world);

    let id3 = Entity::builder()
        .set_default(child_of(id1))
        .spawn(&mut world);

    let entity = world.entity_mut(id3).unwrap();

    assert_eq!(entity.relations(child_of).map(|v| v.0).collect_vec(), [id1]);

    world.set(id3, child_of(id2), ()).unwrap();

    let entity = world.entity_mut(id3).unwrap();
    assert_eq!(entity.relations(child_of).map(|v| v.0).collect_vec(), [id2])
}

#[test]
#[cfg(feature = "flume")]
fn relations_mut() {
    component! {
        relationship(id): f32,
    }

    let mut world = World::new();

    let (changed_tx, changed_rx) = flume::unbounded();

    world.subscribe(changed_tx.filter(|_kind, data| data.key.id() == relationship.id()));

    let id1 = Entity::builder()
        .set(name(), "id1".into())
        .spawn(&mut world);

    let id2 = Entity::builder()
        .set(name(), "id2".into())
        .set(relationship(id1), 1.0)
        .spawn(&mut world);

    let id3 = Entity::builder()
        .set(name(), "id3".into())
        .set(relationship(id1), 2.0)
        .spawn(&mut world);

    let id4 = Entity::builder()
        .set(name(), "id4".into())
        .set(relationship(id2), 3.0)
        .set(relationship(id1), 4.0)
        .spawn(&mut world);

    assert_eq!(
        changed_rx.drain().collect_vec(),
        [
            Event::added(id2, relationship(id1).key()),
            Event::added(id3, relationship(id1).key()),
            Event::added(id4, relationship(id1).key()),
            Event::added(id4, relationship(id2).key()),
        ]
    );

    Query::new(relations_like_mut(relationship))
        .borrow(&world)
        .for_each(|v| v.for_each(|v| *v.1 *= -1.0));

    assert_eq!(
        Query::new((entity_ids(), relations_like(relationship)))
            .borrow(&world)
            .iter()
            .flat_map(|(id, v)| v.map(move |(target, value)| (id, target, *value)))
            .collect_vec(),
        [
            (id2, id1, -1.0),
            (id3, id1, -2.0),
            (id4, id1, -4.0),
            (id4, id2, -3.0),
        ]
    );

    assert_eq!(
        changed_rx.drain().collect_vec(),
        [
            Event::modified(id2, relationship(id1).key()),
            Event::modified(id3, relationship(id1).key()),
            Event::modified(id4, relationship(id1).key()),
            Event::modified(id4, relationship(id2).key()),
        ]
    );
}
