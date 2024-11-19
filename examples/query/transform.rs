use flax::{
    components::{child_of, name},
    filter::All,
    *,
};
use glam::{vec3, Vec3};
use std::fmt::Write;
use tracing_subscriber::{prelude::*, EnvFilter};
use tracing_tree::HierarchicalLayer;

fn main() {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(HierarchicalLayer::default().with_indent_lines(true))
        .init();

    let mut world = World::new();

    // ANCHOR: init
    component! {
        world_position: Vec3,
        position: Vec3,
    }

    let _root = Entity::builder()
        .set(name(), "root".into())
        .set_default(world_position())
        .set(position(), vec3(0.0, 1.0, 0.0))
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "root.child.1".into())
                .set_default(world_position())
                .set(position(), vec3(1.0, 0.0, 0.0))
                .attach(
                    child_of,
                    Entity::builder()
                        .set(name(), "root.child.1.1".into())
                        .set_default(world_position())
                        .set(position(), vec3(0.0, 4.0, 0.0)),
                ),
        )
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "root.child.2".into())
                .set_default(world_position())
                .set(position(), vec3(0.0, 0.5, 0.0)),
        )
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "root.child.3".into())
                .set_default(world_position())
                .set(position(), vec3(0.0, -1.0, 0.0)),
        )
        .spawn(&mut world);
    // ANCHOR_END: init

    // ANCHOR: systems

    let update_world_position = System::builder()
        .with_query(
            Query::new((world_position().as_mut(), position())).with_strategy(Dfs::new(child_of)),
        )
        .build(
            |mut query: DfsBorrow<(ComponentMut<Vec3>, Component<Vec3>), All, ()>| {
                query.traverse(&Vec3::ZERO, |(world_pos, &pos), _, &parent_pos| {
                    *world_pos = pos + parent_pos;
                    *world_pos
                });
            },
        );

    let mut buf = String::new();
    let print_hierarchy = System::builder()
        .with_query(
            Query::new((name(), position(), world_position())).with_strategy(Dfs::new(child_of)),
        )
        .build(move |mut query: DfsBorrow<_, _, _>| {
            query.traverse(&0usize, |(name, pos, world_pos), _, depth| {
                let indent = depth * 4;
                writeln!(
                    buf,
                    "{:indent$}{name}: {pos} {world_pos}",
                    "",
                    indent = indent,
                )
                .unwrap();
                depth + 1
            });

            tracing::info!("{buf}");
        });

    let mut schedule = Schedule::new()
        .with_system(update_world_position)
        .with_system(print_hierarchy);

    schedule.execute_seq(&mut world).unwrap();

    // ANCHOR_END: systems
}
