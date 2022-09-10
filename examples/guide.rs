mod serialize;

use flax::{component, components::name, World};
use itertools::Itertools;

pub fn main() -> color_eyre::Result<()> {
    // ANCHOR: world
    let mut world = World::new();
    // ANCHOR_END: world

    // ANCHOR: spawn
    let id = world.spawn();

    if world.is_alive(id) {
        println!("It is alive!");
    }
    // ANCHOR_END: spawn

    // ANCHOR: despawn
    world.despawn(id)?;

    if world.is_alive(id) {
        println!("We've got a zombie on our hands");
    }

    // ANCHOR_END: despawn
    // ANCHOR: spawn_many
    let ids = world.spawn_many().take(10).collect_vec();
    println!("ids: {ids:?}");
    // ANCHOR_END: spawn_many

    // ANCHOR: component_decl

    component! {
        /// Represents the position in the world
        position: (f32, f32),
    }

    // ANCHOR_END: component_decl

    // ANCHOR: world_component_access
    let id = world.spawn();

    // Add a component to `id`
    world.set(id, position(), (1.0, 4.0))?;

    {
        let val = world.get(id, position())?;

        println!("The entity is at: {val:?}");
    }

    // This will overwrite the previous value
    world.set(id, position(), (1.0, 4.5))?;

    {
        // Mutate the component
        let mut pos = world.get_mut(id, position())?;
        pos.1 += 1.0;
    }

    println!("The entity is now at: {:?}", world.get(id, position())?);

    // ANCHOR_END: world_component_access
    // ANCHOR: component_meta

    component! {
        /// An entity's health.
        /// Provides the Debug bundle, which adds the `debug_visitor` component.
        health: f32 => [ flax::Debug ],
    }

    // After adding the component, the associate metadata of the `health`
    // component is added to the world.
    world.set(id, health(), 100.0)?;

    let component_name = world.get(health().id(), name())?;
    println!("The name of the component is {component_name:?}");

    // Print the state of the world
    println!("World: {world:#?}");
    // Prints:
    //
    // World: {
    //     1v2: {},
    //     2v1: {},
    //     3v1: {},
    //     4v1: {},
    //     5v1: {},
    //     6v1: {},
    //     7v1: {},
    //     8v1: {},
    //     9v1: {},
    //     10v1: {},
    //     11v1: {
    //         "position": _,
    //         "health": 100.0,
    //     },
    // }

    // ANCHOR_END: component_meta

    Ok(())
}
