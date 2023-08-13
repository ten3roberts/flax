use flax::{
    component, entity_ids, BoxedSystem, CommandBuffer, Component, Debuggable, Entity, FetchExt,
    Mutable, Query, QueryBorrow, Schedule, System, World,
};
use glam::{vec2, Vec2};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn main() -> anyhow::Result<()> {
    let mut world = World::new();

    component! {
        position: Vec2 => [ Debuggable ],
        health: f32 => [ Debuggable ],
        distance: f32 => [ Debuggable ],
    }

    Entity::builder()
        .set(position(), vec2(1.0, 4.0))
        .set(health(), 75.0)
        .spawn(&mut world);

    Entity::builder()
        .set(position(), vec2(-1.0, 4.0))
        .set(health(), 75.0)
        .spawn(&mut world);

    #[allow(unused_variables)]
    {
        // ANCHOR: system_basic
        let update_distance = System::builder()
            .with_name("update_distance")
            .with_query(Query::new((entity_ids(), position(), distance().as_mut())))
            .build(
                |mut query: QueryBorrow<(_, Component<Vec2>, Mutable<f32>), _>| {
                    for (id, pos, dist) in &mut query {
                        println!("Updating distance for {id} with position: {pos:?}");
                        *dist = pos.length();
                    }
                },
            );
        // ANCHOR_END: system_basic
    }
    {
        #![allow(dead_code)]
        // ANCHOR: system_func
        fn update_distance_system() -> BoxedSystem {
            System::builder()
                .with_name("update_distance")
                .with_query(Query::new((entity_ids(), position(), distance().as_mut())))
                .build(
                    |mut query: QueryBorrow<(_, Component<Vec2>, Mutable<f32>), _>| {
                        for (id, pos, dist) in &mut query {
                            println!("Updating distance for {id} with position: {pos:?}");
                            *dist = pos.length();
                        }
                    },
                )
                .boxed()
        }
    }
    let mut update_distance = update_distance_system();

    // ANCHOR_END: system_func

    update_distance.run_on(&mut world)?;

    // ANCHOR: add_missing
    fn add_distance_system() -> BoxedSystem {
        let query = Query::new(entity_ids())
            .with(position())
            .without(distance());

        System::builder()
            .with_cmd_mut()
            .with_query(query)
            .build(
                |cmd: &mut CommandBuffer, mut query: QueryBorrow<'_, flax::EntityIds, _>| {
                    for id in &mut query {
                        cmd.set(id, distance(), 0.0);
                    }
                },
            )
            .boxed()
    }
    // ANCHOR_END: add_missing

    // ANCHOR: for_each
    fn update_distance_system() -> BoxedSystem {
        System::builder()
            .with_name("update_distance")
            .with_query(Query::new((entity_ids(), position(), distance().as_mut())))
            .for_each(|(id, pos, dist)| {
                println!("Updating distance for {id} with position: {pos:?}");
                *dist = pos.length();
            })
            .boxed()
    }

    // ANCHOR_END: for_each

    // ANCHOR: schedule
    /// Despawn all entities with a distance > 50
    fn despawn_system() -> BoxedSystem {
        System::builder()
            .with_name("delete_outside_world")
            .with_query(Query::new((entity_ids(), distance())).filter(distance().gt(50.0)))
            .with_cmd_mut()
            .build(|mut q: QueryBorrow<_, _>, cmd: &mut CommandBuffer| {
                for (id, &dist) in &mut q {
                    println!("Despawning {id} at: {dist}");
                    cmd.despawn(id);
                }
            })
            .boxed()
    }

    fn inspect_system() -> BoxedSystem {
        System::builder()
            .with_name("debug_world")
            .with_world()
            .build(|_world: &_| {
                // println!("World: {_world:#?}");
            })
            .boxed()
    }

    component! {
        /// Entities with this component will not be moved
        is_static: () => [ flax::Debuggable ],
    }

    fn move_system() -> BoxedSystem {
        System::builder()
            .with_name("move_out")
            .with_query(Query::new(position().as_mut()).filter(is_static().without()))
            .for_each(|pos| {
                let dir = pos.normalize_or_zero();

                *pos += dir;
            })
            .boxed()
    }

    // Since this system will move non static entities out from the origin, they will
    // eventually be despawned

    // Spawn new entities with a random position each frame
    fn spawn_system(mut rng: StdRng, count: usize) -> BoxedSystem {
        System::builder()
            .with_name("spawn")
            .with_cmd_mut()
            .build(move |cmd: &mut CommandBuffer| {
                for _ in 0..count {
                    let pos = vec2(rng.gen_range(-10.0..10.0), rng.gen_range(-10.0..10.0));
                    println!("Spawning new entity at: {pos:?}");
                    Entity::builder()
                        .set(position(), pos)
                        .set_default(distance())
                        .spawn_into(cmd);
                }
            })
            .boxed()
    }

    let rng = StdRng::seed_from_u64(42);

    // Assemble the schedule, takes care of dependency management
    let mut schedule = Schedule::builder()
        .with_system(add_distance_system())
        .flush()
        .with_system(update_distance_system())
        .with_system(despawn_system())
        .with_system(spawn_system(rng, 1))
        .with_system(move_system())
        .with_system(inspect_system())
        .build();

    for i in 0..20 {
        println!("Frame: {i}");
        schedule.execute_par(&mut world)?;
    }

    // ANCHOR_END: schedule

    Ok(())
}
