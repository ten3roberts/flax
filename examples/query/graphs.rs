use flax::{child_of, entity_ids, name, Dfs, DfsRoots, Entity, Query, Topo, World};
use tracing_subscriber::{prelude::*, EnvFilter};
use tracing_tree::HierarchicalLayer;

fn main() {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(HierarchicalLayer::default().with_indent_lines(true))
        .init();

    let mut world = World::new();

    // ANCHOR: builder

    let root = Entity::builder()
        .set(name(), "root".into())
        .attach(
            child_of,
            Entity::builder().set(name(), "root.child.1".into()).attach(
                child_of,
                Entity::builder().set(name(), "root.child.1.1".into()),
            ),
        )
        .attach(
            child_of,
            Entity::builder().set(name(), "root.child.2".into()),
        )
        .attach(
            child_of,
            Entity::builder().set(name(), "root.child.3".into()),
        )
        .spawn(&mut world);

    // ANCHOR_END: builder
    let root2 = Entity::builder()
        .set(name(), "root2".into())
        .attach(
            child_of,
            Entity::builder().set(name(), "root2.child.1".into()),
        )
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "root2.child.2".into())
                .attach(
                    child_of,
                    Entity::builder()
                        .set(name(), "root2.child.2.1".into())
                        .attach(
                            child_of,
                            Entity::builder().set(name(), "root2.child.2.1.1".into()),
                        ),
                )
                .attach(
                    child_of,
                    Entity::builder().set(name(), "root2.child.2.2".into()),
                ),
        )
        .spawn(&mut world);

    tracing::info!("Spawned roots: {root}, {root2}");

    {
        let _span = tracing::info_span!("dfs");
        // ANCHOR: dfs

        let mut query = Query::new((entity_ids(), name())).with_strategy(Dfs::new(child_of, root));

        tracing::info!("Dfs:");
        for (id, name) in query.borrow(&world).iter() {
            tracing::info!(?id, ?name);
        }

        let mut query = Query::new((entity_ids(), name())).with_strategy(DfsRoots::new(child_of));

        tracing::info!("DfsRoots:");
        for (id, name) in query.borrow(&world).iter() {
            tracing::info!(?id, ?name);
        }

        // ANCHOR_END: dfs
    }

    {
        let _span = tracing::info_span!("topo");
        // ANCHOR: topo

        let mut query = Query::new((entity_ids(), name())).with_strategy(Topo::new(child_of));

        for (id, name) in query.borrow(&world).iter() {
            tracing::info!(?id, ?name);
        }

        // ANCHOR_END: topo
    }
}
