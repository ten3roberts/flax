use flax::*;
use itertools::Itertools;
use tracing_subscriber::{prelude::*, registry};
use tracing_tree::HierarchicalLayer;

fn main() -> color_eyre::Result<()> {
    registry().with(HierarchicalLayer::default()).init();
    color_eyre::install()?;

    // ANCHOR: relation_basic
    component! {
        spring_joint(other): f32 => [Debuggable],
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

    // ANCHOR_END: relation_basic

    // ANCHOR: many_to_many
    let parent2 = Entity::builder()
        .set(name(), "Parent2".into())
        .spawn(&mut world);

    world.set(child1, child_of(parent2), ())?;

    tracing::info!("World: {world:#?}");

    // Connect child1 with two entities via springs of different strength
    world.set(child1, spring_joint(child2), 1.5)?;
    world.set(child1, spring_joint(parent2), 7.4)?;

    tracing::info!(
        "Connections from child1({child1}): {:?}",
        Query::new(relations_like(spring_joint))
            .borrow(&world)
            .get(child1)?
            .collect_vec()
    );

    // ANCHOR_END: many_to_many
    // ANCHOR: query

    let children_of_parent = Query::new(entity_ids())
        .with(child_of(parent))
        .borrow(&world)
        .iter()
        .collect_vec();

    tracing::info!("Children: {children_of_parent:?}");

    let all_children = Query::new(entity_ids())
        .filter(child_of.with_relation())
        .borrow(&world)
        .iter()
        .collect_vec();

    tracing::info!("Children: {all_children:?}");

    let roots = Query::new(entity_ids())
        .filter(child_of.without_relation())
        .borrow(&world)
        .iter()
        .collect_vec();

    tracing::info!("Roots: {roots:?}");

    // ANCHOR_END: query

    // ANCHOR: lifetime

    tracing::info!(
        "has relation to: {parent2}: {}",
        world.has(child1, child_of(parent2))
    );

    world.despawn(parent2)?;

    tracing::info!(
        "has relation to: {parent2}: {}",
        world.has(child1, child_of(parent2))
    );

    tracing::info!("World: {world:#?}");
    world.despawn_recursive(parent, child_of)?;

    tracing::info!("World: {world:#?}");
    // ANCHOR_END: lifetime

    Ok(())
}
