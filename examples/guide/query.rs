use flax::{
    component, entities, CmpExt, CommandBuffer, Component, Debug, Mutable, Query, QueryData,
    Schedule, System, SystemContext, World, Write,
};

fn main() -> color_eyre::Result<()> {
    tracing_subscriber::fmt::init();
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

    for (pos, health) in &mut query.prepare(&world) {
        println!("pos: {pos:?}, health: {health}");
    }

    // ANCHOR_END: query_simple
    // ANCHOR: query_modified

    component! {
        /// Distance to origin
        distance: f32,
    }

    tracing::info!("Spawning id3");
    let id3 = world.spawn();
    world.set(id3, position(), (5.0, 6.0))?;
    world.set(id3, health(), 5.0)?;

    for id in [id, id2, id3] {
        tracing::info!("Adding distance to {id}");
        world.set(id, distance(), 0.0)?;
    }

    let mut query = Query::new((entities(), position(), distance().as_mut()))
        .filter(position().modified() & health().gt(0.0));

    tracing::info!("Updating distances");
    for (id, pos, dist) in &mut query.prepare(&world) {
        tracing::info!("Updating distance for {id} with position: {pos:?}");
        *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
    }

    // ANCHOR_END: query_modified

    // ANCHOR: query_repeat

    tracing::info!("Running query again");
    for (id, pos, dist) in &mut query.prepare(&world) {
        tracing::info!("Updating distance for {id} with position: {pos:?}");
        *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
    }
    // ANCHOR_END: query_repeat

    // ANCHOR: query_repeat_reboot

    *world.get_mut(id2, position())? = (8.0, 3.0);

    tracing::info!("... and again");
    for (id, pos, dist) in &mut query.prepare(&world) {
        tracing::info!("Updating distance for {id} with position: {pos:?}");
        *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
    }

    // ANCHOR_END: query_repeat_reboot

    // ANCHOR: system_basic

    let mut update_dist = System::builder()
        .with_name("update distance")
        .with(query)
        .build(
            |mut query: QueryData<(_, Component<(f32, f32)>, Mutable<f32>), _>| {
                for (id, pos, dist) in &mut query.prepare() {
                    tracing::info!("Updating distance for {id} with position: {pos:?}");
                    *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
                }
            },
        );

    update_dist.run_on(&mut world);
    // ANCHOR_END: system_basic

    // ANCHOR: system_for_each
    let mut update_dist = System::builder()
        .with_name("update distance")
        .with(
            Query::new((entities(), position(), distance().as_mut())).filter(position().modified()),
        )
        .for_each(|(id, pos, dist)| {
            tracing::info!("Updating distance for {id} with position: {pos:?}");
            *dist = (pos.0 * pos.0 + pos.1 * pos.1).sqrt();
        });

    for _ in 0..16 {
        update_dist.run_on(&mut world);
    }

    // ANCHOR_END: system_for_each

    // ANCHOR: system_cmd
    /// Despawn all entities with a distance > 10
    let mut despawned = System::builder()
        .with_name("delete outside world")
        .with(Query::new(entities()).filter(distance().gt(10.0)))
        .with_cmd()
        .build(
            |mut query: QueryData<_, _>, mut cmd: Write<CommandBuffer>| {
                for id in &mut query.prepare() {
                    tracing::info!("Despawning {id}");
                    cmd.despawn(id);
                }
            },
        );

    let mut debug_world = System::builder()
        .with_name("debug world")
        .with_world()
        .build(|world: Write<World>| {
            tracing::info!("World: {world:#?}");
        });

    // ANCHOR_END: system_cmd

    // ANCHOR: schedule_basic
    let schedule = Schedule::new()
        .with_system(update_dist)
        .with_system(despawned)
        .with_system(debug_world);

    tracing::info!("Schedule: {schedule:#?}");

    // ANCHOR_END: schedule_basic

    Ok(())
}
