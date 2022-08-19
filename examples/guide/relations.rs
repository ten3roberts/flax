use flax::{Component, ComponentBuffer, Debug, Entity, MetaData, World};
use glam::{vec2, Vec2};
use tracing_subscriber::{prelude::*, registry};
use tracing_tree::HierarchicalLayer;

fn main() -> color_eyre::Result<()> {
    registry().with(HierarchicalLayer::default()).init();

    let mut world = World::new();

    let position: Component<Vec2> = world.spawn_component("position", |info| {
        let mut buf = ComponentBuffer::new();
        <Debug as MetaData<Vec2>>::attach(info, &mut buf);
        buf
    });

    let id = Entity::builder()
        .set(position, vec2(1.0, 6.4))
        .spawn(&mut world);

    tracing::info!("world: {world:#?}");

    // When `position` is despawned, it is removed from all entities.
    // This ensured that dead components never exist
    world.despawn(position.id())?;

    tracing::info!("world: {world:#?}");

    Ok(())
}
