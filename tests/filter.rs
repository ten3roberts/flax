use std::sync::Arc;

use flax::{component, EntityBuilder, FetchExt, Query, World};
use itertools::Itertools;

component! {
    a: f32,
    b: String,
    c: Arc<i32>,
}

#[test]
fn filters() {
    let mut world = World::new();

    let id1 = EntityBuilder::new()
        .set(a(), 0.4)
        .set(b(), "Hello, World!".to_string())
        .spawn(&mut world);

    let shared = Arc::new(829);

    let id2 = EntityBuilder::new()
        .set(b(), "Foo".to_string())
        .set(c(), shared)
        .spawn(&mut world);

    let mut builder = EntityBuilder::new();
    let others = (0..10)
        .map(|i| builder.set(a(), i as f32).spawn(&mut world))
        .collect_vec();

    let mut query = Query::new(a()).filter(a().changed());

    let items = query.iter(&world).collect_vec();

    assert_eq!(items.len(), 0);

    eprintln!("Current change: {}", world.change_tick());
    world.set(id1, a(), 34.0);

    eprintln!("Current change: {}", world.change_tick());

    let items = query.iter(&world).collect_vec();

    assert_eq!(items, &[&34.0]);

    others[3..7].iter().for_each(|id| {
        let mut a = world.get_mut(*id, a()).unwrap();
        *a = -*a;
    });

    let items = query.iter(&world).collect_vec();

    eprintln!("Items: {items:?}");

    assert_eq!(items, &[&-3.0, &-4.0, &-5.0, &-6.0]);
}
