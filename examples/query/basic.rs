use std::f32::consts::TAU;

use flax::*;
use glam::{vec3, Vec3};
use itertools::Itertools;
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing::info_span;
use tracing_subscriber::{prelude::*, registry};
use tracing_tree::HierarchicalLayer;

fn main() {
    registry().with(HierarchicalLayer::new(4)).init();

    component! {
        health: f32,
        position: Vec3,
        velocity: Vec3,
        player: (),
    }

    let mut world = World::new();

    // Spawn a single player, and mark it with the `player` component
    Entity::builder()
        .set(name(), "player".into())
        .set(health(), 100.0)
        .set_default(position())
        .set_default(velocity())
        .set_default(player())
        .spawn(&mut world);

    // Enemies
    (0..10)
        .map(|i| {
            let theta = TAU * i as f32 / 10.0;
            Entity::builder()
                .set(name(), format!("enemy.{i}"))
                .set(health(), 50.0)
                .set(position(), vec3(5.0 * theta.cos(), 0.0, 5.0 * theta.sin()))
                .set_default(velocity())
                .spawn(&mut world)
        })
        .collect_vec();

    // Spawn rocks
    (0..64)
        .map(|i| {
            let r = 2.0 + i as f32 / 10.0;
            let theta = TAU * i as f32 / 10.0;
            Entity::builder()
                .set(name(), format!("rock.{i}"))
                .set(position(), vec3(r * theta.cos(), 0.0, r * theta.sin()))
                .spawn(&mut world)
        })
        .collect_vec();

    {
        let _span = info_span!("query_name").entered();

        // ANCHOR: query_name

        let mut query = Query::new(name());

        for name in &mut query.borrow(&world) {
            tracing::info!("Entity: {name:?}");
        }

        // ANCHOR_END: query_name
    }

    {
        let _span = info_span!("query_tuple").entered();
        // ANCHOR: query_tuple

        let mut query = Query::new((name(), position(), health()));

        for (name, pos, health) in &mut query.borrow(&world) {
            tracing::info!("Entity: {name:?} pos: {pos}, health: {health}");
        }

        // ANCHOR_END: query_tuple
    }

    let mut rng = StdRng::seed_from_u64(42);

    {
        let _span = info_span!("query_mut").entered();
        // ANCHOR: query_mut

        fn lightning_strike(world: &World, rng: &mut StdRng) {
            let mut query = Query::new(health().as_mut());
            for h in &mut query.borrow(world) {
                // &mut f32
                *h -= rng.gen_range(10.0..20.0);
            }
        }

        lightning_strike(&world, &mut rng);

        // ANCHOR_END: query_mut
    }

    {
        let _span = info_span!("query_opt").entered();
        // ANCHOR: query_opt

        let mut query = Query::new((name(), position(), health().opt()));

        (&mut query.borrow(&world)).into_iter().for_each(
            |(name, pos, health): (&String, &Vec3, Option<&f32>)| {
                tracing::info!("Entity: {name:?} pos: {pos}, health: {health:?}");
            },
        );

        // ANCHOR_END: query_opt
    }

    {
        let _span = info_span!("query_with").entered();
        // ANCHOR: query_with

        let mut query = Query::new((name(), health())).filter(player().with());

        let mut borrow = query.borrow(&world);

        if let Some((name, health)) = borrow.iter().next() {
            tracing::info!("The player {name} is alive and well at {health} health");
        } else {
            tracing::info!("The player seems to have perished");
        }

        // ANCHOR_END: query_with
    }

    {
        let _span = info_span!("query_without").entered();
        // ANCHOR: query_without

        let mut query = Query::new((name(), health())).filter(player().without());

        for (name, health) in &mut query.borrow(&world) {
            tracing::info!("Npc: {name} at {health} health");
        }

        // ANCHOR_END: query_without
    }

    {
        let _span = info_span!("query_combinators").entered();
        // ANCHOR: query_combinators

        let mut query =
            Query::new((name(), health().opt())).filter(player().with() | health().without());

        for (name, health) in &mut query.borrow(&world) {
            if let Some(health) = health {
                tracing::info!("{name} at {health}");
            } else {
                tracing::info!("{name} (immortal)");
            }
        }

        // ANCHOR_END: query_combinators
    }

    {
        let _span = info_span!("query_cmp").entered();
        // ANCHOR: query_cmp

        let mut query = Query::new(name()).filter(health().without() | health().ge(35.0));
        for name in &mut query.borrow(&world) {
            tracing::info!("{name} is still standing strong");
        }

        // ANCHOR_END: query_cmp
    }
}
