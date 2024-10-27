use flax::{
    component, entity_ids, CommandBuffer, Component, Debuggable, Entity, EntityBorrow, FetchExt,
    Mutable, Query, QueryBorrow, Schedule, System, World,
};
use glam::{vec2, Vec2};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn main() -> anyhow::Result<()> {
    let mut world = World::new();

    component! {
        position: Vec2 => [ Debuggable ],
        health: f32 => [ Debuggable ],
    }

    // ANCHOR: query_simple
    // Spawn two entities
    let id = Entity::builder()
        .set(position(), vec2(1.0, 4.0))
        .set(health(), 75.0)
        .spawn(&mut world);

    let id2 = Entity::builder()
        .set(position(), vec2(-1.0, 4.0))
        .set(health(), 75.0)
        .spawn(&mut world);

    let mut query = Query::new((position(), health()));

    for (pos, health) in &mut query.borrow(&world) {
        println!("pos: {pos:?}, health: {health}");
    }

    // ANCHOR_END: query_simple
    // ANCHOR: query_modified

    component! {
        /// Distance to origin
        distance: f32 => [ flax::Debuggable ],
    }

    println!("Spawning id3");
    let id3 = world.spawn();
    world.set(id3, position(), vec2(5.0, 6.0))?;
    world.set(id3, health(), 5.0)?;

    for id in [id, id2, id3] {
        println!("Adding distance to {id}");
        world.set(id, distance(), 0.0)?;
    }

    let mut query = Query::new((entity_ids(), position(), distance().as_mut()))
        .with_filter(position().modified() & health().gt(0.0));

    println!("Updating distances");
    for (id, pos, dist) in &mut query.borrow(&world) {
        println!("Updating distance for {id} with position: {pos:?}");
        *dist = pos.length();
    }

    // ANCHOR_END: query_modified

    // ANCHOR: query_repeat

    println!("Running query again");
    for (id, pos, dist) in &mut query.borrow(&world) {
        println!("Updating distance for {id} with position: {pos:?}");
        *dist = pos.length();
    }
    // ANCHOR_END: query_repeat

    // ANCHOR: query_repeat_reboot

    *world.get_mut(id2, position())? = vec2(8.0, 3.0);

    println!("... and again");
    for (id, pos, dist) in &mut query.borrow(&world) {
        println!("Updating distance for {id} with position: {pos:?}");
        *dist = pos.length();
    }

    // ANCHOR_END: query_repeat_reboot

    #[allow(unused_variables)]
    {
        // ANCHOR: shorthand
        // Instead of this:
        let query = Query::new((position(), health(), distance()))
            .with_filter(position().modified() & health().modified());

        // Do this:
        let query = Query::new((position().modified(), health().modified(), distance()));
        // ANCHOR_END: shorthand
    }

    // ANCHOR: system_basic

    let mut update_distance = System::builder()
        .with_name("update_distance")
        .with_query(query)
        .build(
            |mut query: QueryBorrow<(_, Component<Vec2>, Mutable<f32>), _>| {
                for (id, pos, dist) in &mut query {
                    println!("Updating distance for {id} with position: {pos:?}");
                    *dist = pos.length();
                }
            },
        );

    update_distance.run(&mut world);
    // ANCHOR_END: system_basic

    // ANCHOR: system_for_each
    let mut update_dist = System::builder()
        .with_name("update_distance")
        .with_query(
            Query::new((entity_ids(), position(), distance().as_mut()))
                .with_filter(position().modified()),
        )
        .for_each(|(id, pos, dist)| {
            tracing::debug!("Updating distance for {id} with position: {pos:?}");
            *dist = pos.length();
        });

    for _ in 0..16 {
        update_dist.run(&mut world);
    }

    // ANCHOR_END: system_for_each

    // ANCHOR: system_cmd
    // Despawn all entities with a distance > 50
    // ANCHOR: schedule_basic
    let despawn = System::builder()
        .with_name("delete_outside_world")
        .with_query(Query::new((entity_ids(), distance())).with_filter(distance().gt(50.0)))
        .with_cmd_mut()
        .build(|mut q: QueryBorrow<_, _>, cmd: &mut CommandBuffer| {
            for (id, &dist) in &mut q {
                println!("Despawning {id} at: {dist}");
                cmd.despawn(id);
            }
        });

    let debug_world = System::builder()
        .with_name("debug_world")
        .with_world()
        .build(|world: &_| {
            tracing::debug!("World: {world:?}");
        });

    // ANCHOR_END: system_cmd

    component! {
        is_static: () => [ flax::Debuggable ],
    }

    // Spawn 150 static entities, which wont move
    let mut rng = StdRng::seed_from_u64(42);

    for _ in 0..150 {
        let pos = vec2(rng.gen_range(-5.0..5.0), rng.gen_range(-5.0..5.0));
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
        .with_query(Query::new(position().as_mut()).with_filter(is_static().without()))
        .for_each(|pos| {
            let dir = pos.normalize_or_zero();

            *pos += dir;
        });

    // Spawn new entities with a random position each frame
    let spawn = System::builder().with_name("spawner").with_cmd_mut().build(
        move |cmd: &mut CommandBuffer| {
            for _ in 0..100 {
                let pos = vec2(rng.gen_range(-10.0..10.0), rng.gen_range(-10.0..10.0));
                println!("Spawning new entity at: {pos:?}");
                Entity::builder()
                    .set(position(), pos)
                    .set_default(distance())
                    .spawn_into(cmd);
            }
        },
    );

    let mut frame_count = 0;

    // Count the number of entities in the world and log it
    let count = System::builder()
        .with_name("count")
        .with_query(Query::new(()))
        .build(move |mut query: QueryBorrow<()>| {
            let count: usize = query.iter_batched().map(|v| v.len()).sum();
            println!("[{frame_count}]: {count}");
            frame_count += 1;
        });

    // Assemble the schedule, takes care of dependency management
    let mut schedule = Schedule::builder()
        .with_system(update_dist)
        .with_system(despawn)
        .with_system(spawn)
        .with_system(move_out)
        .with_system(debug_world)
        .with_system(count)
        .build();

    println!("{schedule:#?}");

    for i in 0..20 {
        println!("Frame: {i}");
        println!("Batches: {:#?}", schedule.batch_info(&world));
        schedule.execute_par(&mut world)?;
    }

    // ANCHOR_END: schedule_basic

    // ANCHOR: entity_query
    component! {
        window_width: f32,
        window_height: f32,
        allow_vsync: bool,

        /// A static entity, which is always alive
        resources,
    }

    Entity::builder()
        .set(window_width(), 800.0)
        .set(window_height(), 600.0)
        .set(allow_vsync(), false)
        // Since `resources` is static, it is not required to spawn it
        .append_to(&mut world, resources())
        .unwrap();

    let mut query = Query::new((window_width(), window_height(), allow_vsync()))
        // Change the query strategy to only iterate the `resources` entity
        .entity(resources());

    let mut borrow = query.borrow(&world);
    let (width, height, vsync) = borrow.get().unwrap();
    println!("width: {width} height: {height}, vsync: {vsync}");

    // ANCHOR_END: entity_query

    drop(borrow);

    // ANCHOR: entity_query_system

    let mut window_system = System::builder()
        .with_query(query)
        .build(|mut q: EntityBorrow<_>| {
            if let Ok((width, height, allow_vsync)) = q.get() {
                println!(
                    "Config changed width: {width}, height: {height}, allow_vsync: {allow_vsync}"
                );
            } else {
                println!("No config change");
            }
        });

    window_system.run(&mut world);
    window_system.run(&mut world);
    world.set(resources(), window_height(), 720.0)?;
    window_system.run(&mut world);

    // ANCHOR_END: entity_query_system

    Ok(())
}
