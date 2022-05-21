use flax::{component, Query, World};
use itertools::Itertools;

#[test]
fn creation() {
    component! {
        a: i32,
        b: String,
    }

    let mut world = World::new();

    let e = world.spawn();
    world.insert(e, a(), 5);

    assert!(world.is_alive(e));
    world.despawn(e);
    assert!(!world.is_alive(e));
}

#[test]
fn query() {
    component! {
        a: i32,
        b: String,
    }

    let mut world = World::new();

    let id1 = world.spawn();
    let id2 = world.spawn();
    let id3 = world.spawn();

    world.insert(id1, a(), 4);
    world.insert(id2, a(), 9);
    world.insert(id3, a(), 8);
    world.insert(id3, b(), "foo".to_string());

    let mut query = Query::new(a());
    let items = query
        .iter(&world)
        .inspect(|&&v| println!("{v}"))
        .copied()
        .sorted()
        .collect_vec();

    assert_eq!(items, [4, 8, 9])
}
