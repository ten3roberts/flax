use flax::*;
use glam::{vec3, Vec3};
use tracing_subscriber::{prelude::*, EnvFilter};
use tracing_tree::HierarchicalLayer;

fn main() {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(HierarchicalLayer::default().with_indent_lines(true))
        .init();

    let mut world = World::new();

    // ANCHOR: builder

    component! {
        world_position: Vec3,
        position: Vec3,
    }

    let root = Entity::builder()
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
                        .set_default(world_position()),
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

    let update_world_position = System::builder()
        .with(
            Query::new((world_position().as_mut(), position().opt_or_default()))
                .with_strategy(Dfs::new(child_of)),
        )
        .build(
            |mut query: DfsBorrow<(Mutable<Vec3>, Component<Vec3>), All, ()>| {
                query.traverse(&Vec3::ZERO, |(world_pos, &pos), _, &parent_pos| {
                    *world_pos = pos + parent_pos;
                    *world_pos
                });
            },
        );

    // let mut buf = String::new();
    // let print_hierarchy = System::builder()
    //     .with(Query::new((name(), world_position())))
    //     .build(|mut query: DfsBorrow<_, _, _>| {
    //         query.traverse(0usize, |(name, world_pos), _, depth| {
    //             write!(buf, "{:indent$}{name}: {world_pos}", "", indent = depth * 4)
    //         });
    //     });
}
