use eyre::Context;
use flax::*;
use glam::{vec2, Mat4, Vec2};
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing_subscriber::{prelude::*, registry};
use tracing_tree::HierarchicalLayer;

fn main() -> color_eyre::Result<()> {
    registry().with(HierarchicalLayer::default()).init();
    color_eyre::install()?;

    // ANCHOR: basics

    component! {
        position: Vec2,
    }

    let mut world = World::new();

    let mut cmd = CommandBuffer::new();

    cmd.spawn(Entity::builder().set(name(), "a".into()));

    cmd.apply(&mut world)?;

    let id = Query::new(entity_ids())
        .filter(name().eq("a".into()))
        .borrow(&world)
        .iter()
        .next()
        .ok_or(eyre::eyre!("Missing entity"))?;

    cmd.set(id, position(), vec2(32.0, 2.6));
    let id2 = world.spawn();

    cmd.spawn(
        EntityBuilder::new()
            .set(name(), "b".into())
            .set(position(), vec2(4.6, 8.4))
            .with_id(id2),
    );

    cmd.remove(id2, position());

    cmd.apply(&mut world)?;

    cmd.set(id2, child_of(id), ());

    // Execute this function when the commandbuffer is applied
    cmd.defer(move |w| {
        w.despawn_recursive(id, child_of)?;
        Ok(())
    });

    cmd.apply(&mut world)?;
    // ANCHOR_END: basics

    // ANCHOR: schedule
    component! {
        world_matrix: Mat4 => [Debug],
    }

    // Make sure there are always 64 entities in the world
    let mut rng = StdRng::seed_from_u64(42);
    let spawner = System::builder()
        .with_name("spawn_entities")
        .with(Query::new(()))
        .write::<CommandBuffer>()
        .build(move |mut q: QueryData<()>, mut cmd: Write<CommandBuffer>| {
            let count = q.borrow().count();

            for _ in count..64 {
                tracing::info!("Spawning new entity");
                cmd.spawn(
                    Entity::builder()
                        .set(name(), "entity".to_string())
                        .set(position(), rng.gen()),
                );
            }
        });

    // Ensure a world matrix to each entity with a position
    let add_world_matrix = System::builder()
        .with_name("add_world_matrix")
        .with(Query::new((entity_ids(), position())).without(world_matrix()))
        .write::<CommandBuffer>()
        .build(
            |mut q: QueryData<(Entities, Component<Vec2>), _>, mut cmd: Write<CommandBuffer>| {
                for (id, pos) in &mut q.borrow() {
                    tracing::info!("Adding world matrix to {id}");
                    cmd.set(id, world_matrix(), Mat4::from_translation(pos.extend(0.0)));
                }
            },
        );

    // Update the world matrix if position changes
    let update_world_matrix = System::builder()
        .with_name("update_world_matrix")
        .with(
            Query::new((entity_ids(), position(), world_matrix().as_mut()))
                .filter(position().modified()),
        )
        .for_each(|(id, pos, ltw)| {
            tracing::info!("Updating world matrix for {id}");
            *ltw = Mat4::from_translation(pos.extend(0.0));
        });

    let mut schedule = Schedule::builder()
        .with_system(spawner)
        .flush()
        .with_system(add_world_matrix)
        .flush()
        .with_system(update_world_matrix)
        .build();

    schedule
        .execute_par(&mut world)
        .wrap_err("Failed to run schedule")?;

    // ANCHOR_END: schedule

    Ok(())
}
