use flax::*;
use itertools::Itertools;

#[test]
fn relations() -> color_eyre::Result<()> {
    component! {
        child_of(parent): () => [Debug],
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

    world.set(child1, child_of(parent2), ())?;

    assert_eq!(world.get(child1, child_of(parent2)).as_deref(), Ok(&()));

    let children = Query::new(entities())
        .with(child_of(parent))
        .iter(&world)
        .iter()
        .sorted()
        .collect_vec();

    assert_eq!(children, [child1, child2]);
    tracing::info!("Children: {children:?}");

    let parents = Query::new(entities())
        .filter(child_of.without())
        .iter(&world)
        .iter()
        .collect_vec();

    assert_eq!(parents, [parent, parent2]);
    assert!(world.has(child1, child_of(parent2)));

    world.despawn(parent2)?;

    assert!(!world.has(child1, child_of(parent2)));
    assert!(world.has(child1, child_of(parent)));

    world.despawn_recursive(parent, child_of)?;

    assert!(!world.is_alive(child1));
    assert!(!world.is_alive(child2));

    tracing::info!("World: {world:#?}");

    world.despawn_many(All);

    assert_eq!(
        Query::new(()).iter(&world).count(),
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

    assert_eq!(Query::new(child_of(root)).iter(&world).count(), 2);
    assert_eq!(
        Query::new(()).filter(child_of.with()).iter(&world).count(),
        3
    );

    Ok(())
}
