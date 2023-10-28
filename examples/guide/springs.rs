use flax::{component, components::name, entity_ids, Dfs, Entity, FetchExt, Query, World};
use glam::{vec2, Vec2};
use tracing_subscriber::{prelude::*, registry};
use tracing_tree::HierarchicalLayer;

fn main() {
    registry().with(HierarchicalLayer::default()).init();

    let mut world = World::new();

    // ANCHOR: main
    struct Spring {
        strength: f32,
        length: f32,
    }

    impl Spring {
        fn new(strength: f32, length: f32) -> Self {
            Self { strength, length }
        }
    }
    component! {
        spring_joint(id): Spring,
        position: Vec2,
    }

    let id1 = Entity::builder()
        .set(name(), "a".into())
        .set(position(), vec2(1.0, 4.0))
        .spawn(&mut world);

    // Connect id2 to id1 with a spring of strength 2.0
    let id2 = Entity::builder()
        .set(name(), "b".into())
        .set(spring_joint(id1), Spring::new(2.0, 1.0))
        .set(position(), vec2(2.0, 0.0))
        .spawn(&mut world);

    let _id3 = Entity::builder()
        .set(name(), "c".into())
        .set(spring_joint(id1), Spring::new(2.0, 3.0))
        .set(position(), vec2(2.0, 3.0))
        .spawn(&mut world);

    let _id4 = Entity::builder()
        .set(name(), "d".into())
        .set(spring_joint(id2), Spring::new(5.0, 0.5))
        .set(position(), vec2(1.0, 0.0))
        .spawn(&mut world);

    let mut query = Query::new((entity_ids(), name().cloned(), position()))
        .with_strategy(Dfs::new(spring_joint));

    query
        .borrow(&world)
        .traverse(&None, |(id, name, &pos), strength, parent| {
            if let (Some(spring), Some((parent_name, parent_pos))) = (strength, parent) {
                let distance = pos.distance(*parent_pos) - spring.length;
                let force = distance * spring.strength;
                tracing::info!("spring acting with {force:.1}N between {parent_name} and {name}");
            } else {
                tracing::info!(%id, name, "root");
            }

            Some((name, pos))
        });
    // ANCHOR_END: main
}
