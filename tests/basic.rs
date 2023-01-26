use std::sync::Arc;

use flax::{
    component, components::name, debug_visitor, entity_ids, EntityBuilder, Error, Query, World,
};
use itertools::Itertools;

#[test]
fn creation() {
    component! {
        a: i32 => [ flax::Debug ],
        b: String => [ flax::Debug ],
        zst: (),
    }

    let mut world = World::new();

    let id = world.spawn();
    world.set(id, a(), 5).unwrap();

    world.set(id, zst(), ()).unwrap();

    assert!(world.has(a().id(), debug_visitor()));

    assert!(world.is_alive(id));
    world.despawn(id).unwrap();
    assert!(!world.is_alive(id));
}

#[test]
fn query() {
    component! {
        a: i32,
        b: String,
    }

    let mut query = Query::new((entity_ids(), a()));
    let mut world = World::new();
    query.borrow(&world).iter().for_each(|_| {});

    let id1 = world.spawn();
    let id2 = world.spawn();
    let id3 = world.spawn();

    world.set(id1, a(), 4).unwrap();
    world.set(id1, a(), 4).unwrap();
    world.set(id2, a(), 9).unwrap();
    world.set(id3, a(), 8).unwrap();
    world.set(id3, b(), "foo".to_string()).unwrap();

    let items = query
        .borrow(&world)
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
        c: String => [ flax::Debug ],
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
        .set(b(), shared)
        .spawn(&mut world);

    EntityBuilder::new()
        .set(a(), 9)
        .set(c(), "Bar".to_string())
        .spawn(&mut world);

    assert!(world.has(c().id(), debug_visitor()));

    let mut query = Query::new((a(), c()));
    let mut query = query.borrow(&world);
    let components = query.iter().sorted().collect_vec();

    assert_eq!(
        components,
        [
            (&5, &"Foo".to_string()),
            (&9, &"Bar".to_string()),
            (&38, &"Baz".to_string())
        ]
    );

    assert_eq!(Ok((&5, &"Foo".to_string())), query.get(id));
    drop(query);

    {
        let mut query = Query::new((a(), c().as_mut()));
        let mut prepared = query.borrow(&world);
        let items = prepared.get(id).unwrap();
        *items.1 = items.1.repeat(*items.0);
    }

    assert_eq!(
        world.get(id, c()).as_deref(),
        Ok(&"FooFooFooFooFoo".to_string())
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
    let mut query = query.borrow(&world);
    let items = query.into_iter().sorted().collect_vec();

    let expected = ([50; 16]).iter().chain(&[100]).collect_vec();
    assert_eq!(items, expected);
}

#[test]
fn query_view() {
    component! {
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
    let mut prepared = query.borrow(&world);

    // Perform integration
    for (pos, vel) in &mut prepared {
        *pos += vel * 1.0;
    }

    // Random fetch
    assert_eq!(prepared.get(entities[3]), Ok((&mut 0.3, &0.3)));
    assert_eq!(prepared.get(entities[7]), Ok((&mut 0.7, &0.7)));

    // Disjoint
    assert_eq!(
        prepared.get_disjoint([entities[2], entities[8], entities[4]]),
        Ok([(&mut 0.2, &0.2), (&mut 0.8, &0.8), (&mut 0.4, &0.4)])
    );
}

#[test]
#[should_panic]
fn not_disjoint() {
    component! {
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
    let mut prepared = query.borrow(&world);

    prepared
        .get_disjoint([entities[2], entities[8], entities[2]])
        .unwrap();
}
