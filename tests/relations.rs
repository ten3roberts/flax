use flax::{filter::All, *};
use itertools::Itertools;

#[test]
fn relations() {
    component! {
        child_of(parent): () => [Debuggable],
    }

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

    assert_eq!(children, [child1, child2]);
    tracing::info!("Children: {children:?}");

    let parents = Query::new(entity_ids())
        .filter(child_of.without_relation())
        .borrow(&world)
        .iter()
        .collect_vec();

    assert_eq!(parents, [parent, parent2]);
    assert!(world.has(child1, child_of(parent2)));

    world.despawn(parent2).unwrap();

    assert!(!world.has(child1, child_of(parent2)));
    assert!(world.has(child1, child_of(parent)));

    world.despawn_recursive(parent, child_of).unwrap();

    assert!(!world.is_alive(child1));
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
