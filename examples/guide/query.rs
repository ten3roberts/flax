use std::borrow::BorrowMut;

use flax::{
    component, entity_ids, CmpExt, CommandBuffer, Component, Debug, Entity, EntityBorrow,
    EntityQuery, Mutable, Query, QueryBorrow, Schedule, System, SystemBuilder, World,
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing_subscriber::{
    prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter,
};
use tracing_tree::HierarchicalLayer;

fn main() -> color_eyre::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(HierarchicalLayer::default().with_indent_lines(true))
        .init();

    // ANCHOR: query_simple
    let mut world = World::new();

    component! {
        position: (f32, f32) => [ Debug ],
        health: f32 => [ Debug ],
    }

    // Spawn two entities
    let id = world.spawn();

    world.set(id, position(), (1.0, 4.0))?;
    world.set(id, health(), 100.0)?;

    let id2 = world.spawn();

    world.set(id2, position(), (-1.0, 4.0))?;
    world.set(id2, health(), 75.0)?;

    let mut query = Query::new((position(), health()));

    for (pos, health) in &mut query.borrow(&world) {
        println!("pos: {pos:?}, health: {health}");
    }

    // ANCHOR_END: query_simple
    // ANCHOR: query_modified

    component! {
        /// Distance to origin
        distance: f32 => [ flax::Debug ],
    }

    tracing::info!("Spawning id3");
    let id3 = world.spawn();
    world.set(id3, position(), (5.0, 6.0))?;
    world.set(id3, health(), 5.0)?;

    for id in [id, id2, id3] {
        tracing::info!("Adding distance to {id}");
        world.set(id, distance(), 0.0)?;
    }

    let mut query = Query::new((entity_ids(), position(), distance().as_mut()))
        .filter(position().modified() & health().gt(0.0));

    tracing::info!("Updating distances");
    for (id, pos, dist) in &mut query.borrow(&world) {
        tracing::info!("Updating distance for {id} with position: {pos:?}");
        *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
    }

    // ANCHOR_END: query_modified

    // ANCHOR: query_repeat

    tracing::info!("Running query again");
    for (id, pos, dist) in &mut query.borrow(&world) {
        tracing::info!("Updating distance for {id} with position: {pos:?}");
        *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
    }
    // ANCHOR_END: query_repeat

    // ANCHOR: query_repeat_reboot

    *world.get_mut(id2, position())? = (8.0, 3.0);

    tracing::info!("... and again");
    for (id, pos, dist) in &mut query.borrow(&world) {
        tracing::info!("Updating distance for {id} with position: {pos:?}");
        *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
    }

    // ANCHOR_END: query_repeat_reboot

    #[allow(unused_variables)]
    {
        // ANCHOR: shorthand
        // Instead of this:
        let query = Query::new((position(), health(), distance()))
            .filter(position().modified() | health().modified());

        // Do this:
        let query = Query::new((position().modified(), health().modified(), distance()));
        // ANCHOR_END: shorthand
    }

    // ANCHOR: system_basic

    let mut update_dist = System::builder()
        .with_name("update_distance")
        .with(query)
        .build(
            |mut query: QueryBorrow<(_, Component<(f32, f32)>, Mutable<f32>), _>| {
                for (id, pos, dist) in &mut query {
                    tracing::info!("Updating distance for {id} with position: {pos:?}");
                    *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
                }
            },
        );

    update_dist.run_on(&mut world);
    // ANCHOR_END: system_basic

    // ANCHOR: system_for_each
    let mut update_dist = System::builder()
        .with_name("update_distance")
        .with(
            Query::new((entity_ids(), position(), distance().as_mut()))
                .filter(position().modified()),
        )
        .for_each(|(id, pos, dist)| {
            tracing::debug!("Updating distance for {id} with position: {pos:?}");
            *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
        });

    for _ in 0..16 {
        update_dist.run_on(&mut world);
    }

    // ANCHOR_END: system_for_each

    // ANCHOR: system_cmd
    // Despawn all entities with a distance > 50
    // ANCHOR: schedule_basic
    let despawn = System::builder()
        .with_name("delete_outside_world")
        .with(Query::new((entity_ids(), distance())).filter(distance().gt(50.0)))
        .write::<CommandBuffer>()
        .build(|mut q: QueryBorrow<_, _>, cmd: &mut CommandBuffer| {
            for (id, &dist) in &mut q {
                tracing::info!("Despawning {id} at: {dist}");
                cmd.despawn(id);
            }
        });

    let debug_world = System::builder()
        .with_name("debug_world")
        .read::<World>()
        .build(|world: &_| {
            tracing::debug!("World: {world:#?}");
        });

    // ANCHOR_END: system_cmd

    component! {
        is_static: () => [ flax::Debug ],
    }

    // Spawn 150 static entities, which wont move
    let mut rng = StdRng::seed_from_u64(42);

    for _ in 0..150 {
        let pos = (rng.gen_range(-5.0..5.0), rng.gen_range(-5.0..5.0));
        Entity::builder()
            .set(position(), pos)
            .set_default(distance())
            .set_default(is_static())
            .spawn(&mut world);
    }

    // Since this system will move non static entities out from the origin, they will
    // eventually be despawned
    let move_out = System::builder()
        .with_name("move_out")
        .with(Query::new(position().as_mut()).filter(is_static().without()))
        .for_each(|pos| {
            let mag = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();

            let dir = (pos.0 / mag, pos.1 / mag);

            pos.0 += dir.0;
            pos.1 += dir.1;
        });

    // Spawn new entities with a random position each frame
    let spawn = System::builder()
        .with_name("spawner")
        .write::<CommandBuffer>()
        .build(move |cmd: &mut CommandBuffer| {
            for _ in 0..100 {
                let pos = (rng.gen_range(-10.0..10.0), rng.gen_range(-10.0..10.0));
                tracing::info!("Spawning new entity at: {pos:?}");
                Entity::builder()
                    .set(position(), pos)
                    .set_default(distance())
                    .spawn_into(cmd);
            }
        });

    let mut frame_count = 0;

    // Count the number of entities in the world and log it
    let count = System::builder()
        .with_name("count")
        .with(Query::new(()))
        .build(move |mut query: QueryBorrow<()>| {
            let count: usize = query.iter_batched().map(|v| v.len()).sum();
            tracing::info!("[{frame_count}]: {count}");
            frame_count += 1;
        });

    // Assemple the schedule, takes care of dependency management
    let mut schedule = Schedule::builder()
        .with_system(update_dist)
        .with_system(despawn)
        .with_system(spawn)
        .with_system(move_out)
        .with_system(debug_world)
        .with_system(count)
        .build();

    tracing::info!("{schedule:#?}");

    for i in 0..200 {
        tracing::info!("Frame: {i}");
        tracing::info!("Batches: {:#?}", schedule.batch_info(&mut world));
        schedule.execute_par(&mut world)?;
        // sleep(Duration::from_secs_f32(0.1));
    }

    // ANCHOR_END: schedule_basic

    // ANCHOR: entity_query
    component! {

        window_width: f32,
        window_height: f32,
        allow_vsync: bool,

        resources,
    }

    Entity::builder()
        .set(window_width(), 800.0)
        .set(window_height(), 600.0)
        .set(allow_vsync(), false)
        // Since `resources` is static, it is not required to spawn it
        .append_to(&mut world, resources())
        .unwrap();

    let mut query = EntityQuery::new(
        (window_width(), window_height(), allow_vsync()),
        resources(),
    );

    let window_system = System::builder()
        .with(query)
        .build(|mut q: EntityBorrow<_, _>| {});

    // ANCHOR_END: entity_query
    Ok(())
}
