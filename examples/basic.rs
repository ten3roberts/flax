use flax::*;

fn main() {
    // Declare static components
    component! {
      health: f32,
      regen: f32,
      pos: (f32, f32),
      player: (),
      items: Vec<String>,
    }

    let mut world = World::new();

    // Spawn an entity
    EntityBuilder::new()
        .tag(player())
        .set(health(), 50.0)
        .set(pos(), (0.0, 0.0))
        .set(regen(), 1.0)
        .set_default(items())
        .spawn(&mut world);

    let mut query = Query::new(health());
    for health in &mut query.borrow(&world) {
        eprintln!("Health: {health}");
    }

    let mut query = Query::new((health().as_mut(), regen()));

    // Apply health regen for all match entites
    for (health, regen) in &mut query.borrow(&world) {
        *health = (*health + regen).min(100.0);
    }
}
