use std::sync::Arc;

use flax::{component, EntityBuilder, Query, World};
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

#[test]
fn builder() {
    component! {
        a: i32,
        b: Arc<String>,
        c: String,
    }

    let mut world = World::new();

    let shared = Arc::new("Bar".to_string());

    let id = EntityBuilder::new()
        .set(a(), 5)
        .set(b(), shared.clone())
        .set(c(), "Foo".to_string())
        .set(b(), shared.clone())
        .spawn(&mut world);

    EntityBuilder::new()
        .set(a(), 38)
        .set(b(), shared.clone())
        .set(c(), "Baz".to_string())
        .set(b(), shared.clone())
        .spawn(&mut world);

    let id2 = EntityBuilder::new()
        .set(a(), 9)
        .set(c(), "Bar".to_string())
        .spawn(&mut world);

    let mut query = Query::new((a(), c()));
    let components = query.iter(&world).sorted().collect_vec();

    assert_eq!(
        components,
        [
            (&5, &"Foo".to_string()),
            (&9, &"Bar".to_string()),
            (&38, &"Baz".to_string())
        ]
    );
}
