use std::sync::Arc;

use flax::{component, entities, EntityBuilder, Query, World};
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

    let mut query = Query::new((entities(), a()));
    let mut world = World::new();
    query.prepare(&world).iter().for_each(|_| {});

    let id1 = world.spawn();
    let id2 = world.spawn();
    let id3 = world.spawn();

    world.set(id1, a(), 4);
    world.set(id1, a(), 4);
    world.set(id2, a(), 9);
    world.set(id3, a(), 8);
    world.set(id3, b(), "foo".to_string());

    let items = query
        .prepare(&world)
        .iter()
        .map(|(a, b)| (a, *b))
        .inspect(|v| println!("{v:?}"))
        .sorted_by_key(|(id, _)| *id)
        .collect_vec();

    assert_eq!(items, [(id1, 4), (id2, 9), (id3, 8)])
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
    let mut query = query.prepare(&world);
    let components = query.iter().sorted().collect_vec();

    assert_eq!(
        components,
        [
            (&5, &"Foo".to_string()),
            (&9, &"Bar".to_string()),
            (&38, &"Baz".to_string())
        ]
    );

    assert_eq!(Some((&5, &"Foo".to_string())), query.get(id));
    drop(query);

    {
        let mut query = Query::new((a(), c().as_mut()));
        let mut prepared = query.prepare(&world);
        let items = prepared.get(id).unwrap();
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
    let _player = EntityBuilder::new()
        .set(health(), 100)
        .set(alive(), true)
        .tag(player())
        .set(pos(), (4.5, 3.4))
        .spawn(&mut world);

    let _enemies = (0..16)
        .map(|i| {
            EntityBuilder::new()
                .set(health(), 50)
                .set(alive(), true)
                .set(pos(), (-4.0, 3.0 + i as f32))
                .spawn(&mut world)
        })
        .collect_vec();

    let mut query = Query::new(health());
    let mut query = query.prepare(&world);
    let items = query.into_iter().sorted().collect_vec();

    let expected = (&[50; 16]).iter().chain(&[100]).collect_vec();
    assert_eq!(items, expected);
}

#[test]
fn query_view() {
    component! {
        name: String,
        vel: f32,
        pos: f32,
    }
    let mut world = World::new();
    let mut builder = EntityBuilder::new();
    let entities = (0..10)
        .map(|i| {
            builder
                .set(name(), format!("entity_{i}"))
                .set(vel(), (i as f32) * 0.1)
                .set_default(pos())
                .spawn(&mut world)
        })
        .collect_vec();

    let mut query = Query::new((pos().as_mut(), vel()));
    let mut prepared = query.prepare(&world);

    // Perform integration
    for (pos, vel) in &mut prepared {
        *pos += vel * 1.0;
    }

    // Random fetch
    assert_eq!(prepared.get(entities[3]), Some((&mut 0.3, &0.3)));
    assert_eq!(prepared.get(entities[7]), Some((&mut 0.7, &0.7)));

    // Disjoint
    assert_eq!(
        prepared.get_disjoint([entities[2], entities[8], entities[4]]),
        Some([(&mut 0.2, &0.2), (&mut 0.8, &0.8), (&mut 0.4, &0.4)])
    );

    assert_eq!(
        prepared.get_disjoint([entities[2], entities[8], entities[2]]),
        None
    );
}
