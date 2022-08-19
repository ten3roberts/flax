use flax::{components::name, *};
use glam::{Mat4, Quat, Vec3};
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing_subscriber::prelude::*;

fn main() {
    tracing_subscriber::registry()
        .with(tracing_tree::HierarchicalLayer::default())
        .init();

    component! {
        position: Vec3 => [Debug],
        velocity: Vec3 => [Debug],
        mass: f32 => [Debug],
    }

    let mut world = World::new();

    let mut rng = StdRng::seed_from_u64(42);

    let mut builder = Entity::builder();
    // ANCHOR: full_match

    // Entities with mass
    (0..10).for_each(|i| {
        builder
            .set(name(), format!("Entity.{i}"))
            .set(position(), rng.gen::<Vec3>() * 10.0)
            .set(velocity(), rng.gen())
            .set(mass(), rng.gen_range(10..30) as f32)
            .spawn(&mut world);
    });

    (0..10).for_each(|i| {
        builder
            .set(name(), format!("Entity.{i}"))
            .set(position(), rng.gen::<Vec3>() * 10.0)
            .set(velocity(), rng.gen())
            .set(mass(), rng.gen_range(10..30) as f32)
            .spawn(&mut world);
    });

    // Entities without mass
    (0..100).for_each(|i| {
        builder
            .set(name(), format!("Entity.{i}"))
            .set(position(), rng.gen::<Vec3>() * 0.5)
            .set(velocity(), rng.gen())
            .spawn(&mut world);
    });

    // Since this query accessed `position`, `velocity` **and** `mass` only the
    // first group of entities will be matched
    for (pos, vel, mass) in &mut Query::new((position(), velocity(), mass())).iter(&world) {
        tracing::info!("pos: {pos}, vel: {vel}, mass: {mass}");
    }

    // ANCHOR_END: full_match
    // ANCHOR: opt

    // Use an optional fetch to yield an `Option<T>`, works for any query
    for (pos, vel, mass) in &mut Query::new((position(), velocity(), mass().opt())).iter(&world) {
        if mass.is_some() {
            tracing::info!("Has mass");
        }
        tracing::info!("pos: {pos}, vel: {vel}, mass: {mass:?}");
    }

    // ANCHOR_END: opt
    // ANCHOR: physics

    component! {
        rotation: Quat,
        scale: Vec3,
        world_matrix: Mat4 => [Debug],
    }

    let create_world_matrix = System::builder()
        .with_name("add world matrix")
        .with(
            Query::new(entities())
                .with(position())
                .without(world_matrix()),
        )
        .write::<CommandBuffer>()
        .build(
            |mut query: QueryData<EntityFetch, _>, mut cmd: Write<CommandBuffer>| {
                for id in &mut query.iter() {
                    tracing::info!("Adding world matrix to {id}");
                    cmd.set(id, world_matrix(), Mat4::IDENTITY);
                }
            },
        );

    let update_world_matrix = System::builder()
        .with_name("update world matrix")
        .with(
            Query::new((
                entities(),
                world_matrix().as_mut(),
                position(),
                rotation().opt_or_default(),
                scale().opt_or(Vec3::ONE),
            ))
            .filter(position().modified() | rotation().modified() | scale().modified()),
        )
        .build(
            |mut query: QueryData<
                (
                    EntityFetch,
                    Mutable<Mat4>,
                    Component<Vec3>,
                    OptOr<Component<Quat>, Quat>,
                    OptOr<Component<Vec3>, Vec3>,
                ),
                _,
            >| {
                for (id, world_matrix, pos, rot, scale) in &mut query.iter() {
                    tracing::info!("Updating world matrix for: {id}");
                    *world_matrix = Mat4::from_scale_rotation_translation(*scale, *rot, *pos);
                }
            },
        );

    let mut schedule = Schedule::builder()
        .with_system(create_world_matrix)
        .flush()
        .with_system(update_world_matrix)
        .build();

    schedule
        .execute_par(&mut world)
        .expect("Failed to execute schedule");

    schedule
        .execute_par(&mut world)
        .expect("Failed to execute schedule");

    tracing::info!("World: {world:#?}");

    // ANCHOR_END: physics
}
