use std::process::id;

use flax::{component, entities, CmpExt, Debug, Query, World};

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
    Ok(())
}
