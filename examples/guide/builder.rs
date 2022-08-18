use std::process::id;

use flax::{component, components::name, BatchSpawn, Entity, FetchExt, Query, World};
use itertools::Itertools;
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing::info_span;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};

fn main() {
    // ANCHOR: builder
    tracing_subscriber::registry()
        .with(tracing_tree::HierarchicalLayer::default())
        .init();

    component! {
        health: f32,
        position: (f32, f32),
        is_player: (),
    }

    let mut world = World::new();

    // Instead of this
    let player = world.spawn();
    world.set(player, health(), 100.0).unwrap();
    world.set(player, position(), (5.0, 2.3)).unwrap();
    world.set(player, is_player(), ()).unwrap();
    world.set(player, name(), "Player".into()).unwrap();

    world.despawn(player).unwrap();

    // Do this
    let player = Entity::builder()
        .set(health(), 100.0)
        .set(position(), (5.0, 2.3))
        .tag(is_player())
        .set(name(), "Player".into())
        .spawn(&mut world);

    tracing::info!("Player: {player}");
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
    for (name, pos, is_player, health) in &mut query.prepare(&world) {
        tracing::info!("name: {name}, pos: {pos:?}, player: {is_player:?}, health: {health}");
    }

    // Or to only get the non players

    {
        let mut query = Query::new((name(), position(), health())).without(is_player());
        info_span!("enemies");
        for (name, pos, health) in &mut query.prepare(&world) {
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
            (0..).map(|_| (rng.gen_range(-50.0..50.0), rng.gen_range(-50.0..50.0))),
        )
        .expect("Invalid length");

    let trees = trees.spawn(&mut world);

    tracing::info!("Trees: {trees:?}");

    let mut query = Query::new((name(), position()));
    for (name, pos) in &mut query.prepare(&world) {
        tracing::info!("name: {name}, pos: {pos:?}");
    }

    // ANCHOR_END: batch
}
