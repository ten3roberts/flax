use std::sync::Arc;

use flax::{component, EntityBuilder, Query, World};
use itertools::Itertools;

#[test]
fn creation() {
    component! {
        a: i32,
        b: String,
        zst: (),
    }

    let mut world = World::new();

    let id = world.spawn();
    world.set(id, a(), 5);

    world.set(id, zst(), ());

    assert!(world.is_alive(id));
    world.despawn(id);
    assert!(!world.is_alive(id));
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

    world.set(id1, a(), 4);
    world.set(id1, a(), 4);
    world.set(id2, a(), 9);
    world.set(id3, a(), 8);
    world.set(id3, b(), "foo".to_string());

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
        a: usize,
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

    assert_eq!(
        Some(&(&5, &"Foo".to_string())),
        query.get(id, &world).as_deref()
    );

    {
        let query = Query::new((a(), c().mutable()));
        let mut items = query.get(id, &world).unwrap();
        *items.1 = items.1.repeat(*items.0);
    }

    assert_eq!(
        world.get(id, c()).as_deref(),
        Some(&"FooFooFooFooFoo".to_string())
    );
}

#[test]
fn tags() {
    component! {
        health: i32,
        player: (),
        pos: (f32, f32),
        alive: bool,
    }

    let mut world = World::new();
    let player = EntityBuilder::new()
        .set(health(), 100)
        .set(alive(), true)
        .tag(player())
        .set(pos(), (4.5, 3.4))
        .spawn(&mut world);

    let enemies = (0..16)
        .map(|i| {
            EntityBuilder::new()
                .set(health(), 50)
                .set(alive(), true)
                .set(pos(), (-4.0, 3.0 + i as f32))
                .spawn(&mut world)
        })
        .collect_vec();

    let mut query = Query::new(health());
    let items = query.iter(&world).sorted().collect_vec();

    let expected = (&[50; 16]).iter().chain(&[100]).collect_vec();
    assert_eq!(items, expected);
}
