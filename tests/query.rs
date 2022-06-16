use flax::{component, Query, World};

#[test]
fn query_change() {
    component! {
        name: String,
        health: f32,
        pos: (f32, f32),
        // Distance from origin
        distance: f32,
    }

    let mut world = World::new();

    let move_alive = Query::new((name(), pos().as_mut())); //.filter(health().gt(0.0));

    let consumer = Query::new((name(), pos(), distance().as_mut())).filter(pos().modified());

    // Everything which is alive will move a bit
}
