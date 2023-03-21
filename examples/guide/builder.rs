use flax::*;
use itertools::Itertools;
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing::info_span;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};

fn main() {
    tracing_subscriber::registry()
        .with(tracing_tree::HierarchicalLayer::default())
        .init();

    // ANCHOR: builder
    component! {
        health: f32 => [Debuggable],
        position: (f32, f32) => [Debuggable],
        is_player: () => [Debuggable],
    }

    let mut world = World::new();

    // Instead of this
    let player = world.spawn();
    world.set(player, health(), 100.0).unwrap();
    world.set(player, position(), (5.0, 2.3)).unwrap();
    world.set(player, is_player(), ()).unwrap();
    world.set(player, name(), "Player".into()).unwrap();

    tracing::info!("Player: {:#?}", world.format_entities(&[player]));
    world.despawn(player).unwrap();

    // Do this
    let player = Entity::builder()
        .set(health(), 100.0)
        .set(position(), (5.0, 2.3))
        .tag(is_player())
        .set(name(), "Player".into())
        .spawn(&mut world);

    tracing::info!("Player: {:#?}", world.format_entities(&[player]));

    // ANCHOR_END: builder
    // ANCHOR: reuse

    let mut builder = Entity::builder();
    let mut rng = StdRng::seed_from_u64(42);

    let enemies = (0..10)
        .map(|i| {
            builder
                .set(health(), rng.gen_range(50..100) as f32)
                .set(
                    position(),
                    (rng.gen_range(-10.0..10.0), rng.gen_range(-10.0..10.0)),
                )
                .set(name(), format!("Enemy.{i}"))
                .spawn(&mut world)
        })
        .collect_vec();

    tracing::info!("Enemies: {enemies:?}");
    // ANCHOR_END: reuse

    // ANCHOR: query

    let mut query = Query::new((name(), position(), is_player().opt(), health()));
    for (name, pos, is_player, health) in &mut query.borrow(&world) {
        tracing::info!("name: {name}, pos: {pos:?}, player: {is_player:?}, health: {health}");
    }

    // Or to only get the non players

    {
        let mut query = Query::new((name(), position(), health())).without(is_player());
        info_span!("enemies");
        for (name, pos, health) in &mut query.borrow(&world) {
            tracing::info!("name: {name}, pos: {pos:?}, health: {health}");
        }
    }

    // ANCHOR_END: query
    // ANCHOR: batch

    let mut trees = BatchSpawn::new(10000);
    trees
        .set(name(), (0..).map(|i| format!("Tree.{i}")))
        .expect("Invalid length");

    trees
        .set(
            position(),
            (0..).map(|i| {
                let f = i as f32 / 32.0;
                (f.cos() * (1.0 + f / 2.0), f.sin() * (1.0 + f / 2.0))
            }),
        )
        .expect("Invalid length");

    let trees = trees.spawn(&mut world);

    tracing::info!("Trees: {:#?}", world.format_entities(&trees[0..100]));

    // ANCHOR_END: batch
    world.despawn_many(All);
    // ANCHOR: hierarchy

    let id = Entity::builder()
        .set(name(), "parent".into())
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "child1".into())
                .attach(child_of, Entity::builder().set(name(), "child1.1".into())),
        )
        .attach(child_of, Entity::builder().set(name(), "child2".into()))
        .spawn(&mut world);

    tracing::info!("Parent: {id}");

    tracing::info!("World: {world:#?}");

    // ANCHOR_END: hierarchy
}
