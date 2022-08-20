use flax::*;
use itertools::Itertools;
use tracing_subscriber::{prelude::*, registry};
use tracing_tree::HierarchicalLayer;

fn main() -> color_eyre::Result<()> {
    registry().with(HierarchicalLayer::default()).init();
    // ANCHOR: relation_basic
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

    // ANCHOR_END: relation_basic
    // ANCHOR: many_to_many
    let parent_2 = Entity::builder()
        .set(name(), "Parent2".into())
        .spawn(&mut world);

    world.set(child1, child_of(parent_2), ())?;

    tracing::info!("World: {world:#?}");

    let children = Query::new(entities())
        .with(child_of(parent))
        .iter(&world)
        .iter()
        .collect_vec();

    tracing::info!("Children: {children:?}");
    // ANCHOR_END: many_to_many
    // ANCHOR: lifetime

    tracing::info!(
        "has relation to: {parent_2}: {}",
        world.has(child1, child_of(parent_2))
    );

    world.despawn(parent_2)?;

    tracing::info!(
        "has relation to: {parent_2}: {}",
        world.has(child1, child_of(parent_2))
    );

    tracing::info!("World: {world:#?}");
    world.despawn_recursive(parent)?;

    tracing::info!("World: {world:#?}");
    // ANCHOR_END: lifetime

    Ok(())
}
