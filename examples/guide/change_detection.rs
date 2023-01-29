use flax::{
    component, entity_ids, name, CommandBuffer, Entity, FetchExt, Query, QueryBorrow, System,
    SystemFn, World,
};
use glam::{Mat4, Vec3};
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
        armor: f32,
        player: (),
    }

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

    // ANCHOR_END: setup

    let mut rng = StdRng::from_entropy();

    let mut damage_random = System::builder()
        .write::<World>()
        .build(|world: &mut World| {
            let count = rng.gen_range(0..enemies.len());
            let targets = enemies.choose_multiple(&mut rng, count);
            for &enemy in targets {
                if let Ok(mut health) = world.get_mut(enemy, health()) {
                    *health -= 10.0;
                }
            }
        });

    // ANCHOR: health_changes

    let query = Query::new((name(), health().modified()));

    let mut health_changes = System::builder()
        .with(query)
        .build(|mut query: QueryBorrow<_>| {
            info_span!("health_changes");
            for (name, health) in &mut query {
                tracing::info!("{name:?}: is now at {health} health");
            }
        });

    let query = Query::new(entity_ids()).filter(health().modified().le(0.0) & player());
    let mut cleanup = System::builder().with(query);

    // ANCHOR_END: health_changes

    health_changes.run_on(&mut world);
    health_changes.run_on(&mut world);
    damage_random.run_on(&mut world);
    health_changes.run_on(&mut world);
}
