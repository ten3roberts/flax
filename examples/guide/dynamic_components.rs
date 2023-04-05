use flax::{Component, ComponentBuffer, Debug, Entity, MetaData, World};
use glam::{vec2, Vec2};
use tracing_subscriber::{prelude::*, registry};
use tracing_tree::HierarchicalLayer;

fn main() -> anyhow::Result<()> {
    // ANCHOR: custom

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

    world.despawn(id)?;
    // ANCHOR_END: custom

    // ANCHOR: relation

    #[derive(Debug, Clone)]
    struct RelationData {
        // This allows you to add extra data to the child in the relation
        distance: f32,
    }

    let child_of = world.spawn_relation::<RelationData>("child_of", |info| {
        let mut buf = ComponentBuffer::new();
        <Debug as MetaData<RelationData>>::attach(info, &mut buf);
        buf
    });

    let parent = world.spawn();

    let child = Entity::builder()
        .set(child_of(parent), RelationData { distance: 1.0 })
        .spawn(&mut world);

    let data = world.get(child, child_of(parent))?;

    tracing::info!("Relation distance: {:?}", data.distance);

    drop(data);

    world.despawn(parent)?;
    assert!(world.get(child, child_of(parent)).is_err());

    // ANCHOR_END: relation

    Ok(())
}
