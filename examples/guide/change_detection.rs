use std::{thread::sleep, time::Duration};

use flax::{
    component, entity_ids, name, CommandBuffer, Entity, FetchExt, Query, QueryBorrow, Schedule,
    System, World,
};
use itertools::Itertools;
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use tracing::info_span;
use tracing_subscriber::prelude::*;

fn main() {
    tracing_subscriber::registry()
        .with(tracing_tree::HierarchicalLayer::new(4))
        .init();

    // ANCHOR: setup

    let mut world = World::new();

    component! {
        health: f32,
        poison: f32,
        armor: f32,
        player: (),
    }

    // ANCHOR_END: setup
    //
    // ANCHOR: health_changes

    let query = Query::new((name(), health().modified()));

    let health_changes = System::builder()
        .with(query)
        .build(|mut query: QueryBorrow<_>| {
            info_span!("health_changes");
            for (name, health) in &mut query {
                tracing::info!("{name:?}: is now at {health} health");
            }
        });

    // ANCHOR_END: health_changes

    // ANCHOR: damage

    let player_id = Entity::builder()
        .set(name(), "player".into())
        .set(health(), 100.0)
        .set_default(player())
        .spawn(&mut world);

    let enemies = (0..10)
        .map(|i| {
            Entity::builder()
                .set(name(), format!("enemy.{i}"))
                .set(health(), 50.0)
                .spawn(&mut world)
        })
        .collect_vec();

    let all = enemies.iter().copied().chain([player_id]).collect_vec();

    let mut rng = StdRng::from_entropy();

    all.choose_multiple(&mut rng, all.len() / 5)
        .for_each(|&id| {
            world.set(id, poison(), 10.0).unwrap();
        });

    let damage_random = System::builder()
        .write::<World>()
        .build(move |world: &mut World| {
            let count = rng.gen_range(0..enemies.len());
            let targets = all.choose_multiple(&mut rng, count);
            for &enemy in targets {
                if let Ok(mut health) = world.get_mut(enemy, health()) {
                    *health -= 1.0;
                }
            }
        });

    let update_poison = System::builder()
        .with(Query::new((name().opt(), health().as_mut(), poison())))
        .for_each(|(name, health, poison)| {
            *health -= poison;
            tracing::info!("{name:?} suffered {poison} in poison damage");
        });

    // ANCHOR_END: damage

    // ANCHOR: cleanup_system

    let query = Query::new((name().opt(), entity_ids(), player().satisfied()))
        .filter(health().le(0.0).modified());

    let cleanup = System::builder()
        .with_name("cleanup")
        .with(query)
        .write::<CommandBuffer>()
        .build(|mut q: QueryBorrow<_, _>, cmd: &mut CommandBuffer| {
            for (name, id, is_player) in &mut q {
                if is_player {
                    tracing::info!("Player died");
                }
                tracing::info!(name, is_player, "Despawning {id}");
                cmd.despawn(id);
            }
        });

    // ANCHOR_END: cleanup_system

    // ANCHOR: schedule

    let mut schedule = Schedule::new()
        .with_system(damage_random)
        .with_system(update_poison)
        .with_system(health_changes)
        .flush()
        .with_system(cleanup)
        .flush();

    while world.is_alive(player_id) {
        schedule
            .execute_par(&mut world)
            .expect("Failed to run schedule");

        sleep(Duration::from_millis(1000));
    }

    // ANCHOR_END: schedule
}
