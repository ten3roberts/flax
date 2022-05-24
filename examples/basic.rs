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
    let p = EntityBuilder::new()
        .set(health(), 50.0)
        .tag(player())
        .set(pos(), (0.0, 0.0))
        .set(regen(), 1.0)
        .set_default(items())
        .spawn(&mut world);

    let mut query = Query::new((health().mutable(), regen()));

    // Apply health regen for all match entites
    for (health, regen) in query.iter(&world) {
        *health = (*health + regen).min(100.0);
    }
}
