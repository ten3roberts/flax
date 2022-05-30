use std::sync::Arc;

use flax::{component, entities, EntityBuilder, Query, World};
use itertools::Itertools;

component! {
    a: f32,
    b: String,
    c: Arc<i32>,
    d: &'static str,
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

    let mut query = Query::new(a()).filter(a().modified());

    let items = query.iter(&world).collect_vec();

    // All changed entities
    assert_eq!(items.len(), 11);

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

    others[3..5].iter().for_each(|id| {
        let mut a = world.get_mut(*id, a()).unwrap();
        *a = 10.0 * *a;
    });

    let items = query.iter(&world).collect_vec();
    assert_eq!(items, &[&-30.0, &-40.0]);

    // Construct a new interted query

    let mut query = Query::new(a()).filter(a().inserted());

    let items = query
        .iter(&world)
        .copied()
        .sorted_by_key(|v| (v * 256.0) as i64)
        .collect_vec();

    assert_eq!(
        items,
        &[-40.0, -30.0, -6.0, -5.0, 0.0, 1.0, 2.0, 7.0, 8.0, 9.0, 34.0]
    );

    world.set(id2, a(), 29.5);

    let items = query
        .iter(&world)
        .copied()
        .sorted_by_key(|v| (v * 256.0) as i64)
        .collect_vec();

    assert_eq!(items, &[29.5]);

    let mut query = Query::new(entities()).filter(a().removed());

    let items = query.iter(&world).collect_vec();

    assert_eq!(items, []);
    world.remove(id2, a());

    let items = query.iter(&world).collect_vec();

    assert_eq!(items, [id2]);
}

#[test]
fn combinations() {
    let mut world = World::new();

    component! {
        a: i32,
    }

    let mut builder = EntityBuilder::new();
    let ids = (0..100)
        .map(|i| {
            builder.set(a(), i as _);

            builder.set_default(b());

            if i % 3 == 0 {
                builder.get_mut(b()).unwrap().push_str("Fizz");
            }

            if i % 5 == 0 {
                builder.get_mut(b()).unwrap().push_str("Buzz");
            }

            if i % 2 == 0 {
                builder.set(d(), "Foo");
            }

            builder.spawn(&mut world)
        })
        .collect_vec();

    let mut query = Query::new(entities()).filter(a().modified() | b().modified());

    // eprintln!("Items: {:?}", query.iter(&world).sorted().collect_vec());
    assert_eq!(query.iter(&world).sorted().collect_vec(), ids);

    for &id in &ids[50..67] {
        *world.get_mut(id, a()).unwrap() *= -2;
    }

    let items = query.iter(&world).sorted().collect_vec();
    eprintln!("Items: {items:?}");

    assert_eq!(items, ids[50..67]);
    let items = query.iter(&world).sorted().collect_vec();
    assert_eq!(items, []);

    for &id in &ids[20..43] {
        *world.get_mut(id, a()).unwrap() *= -2;
    }

    for &id in &ids[40..89] {
        world.get_mut(id, b()).unwrap().push_str("...");
    }

    let items = query.iter(&world).sorted().collect_vec();

    assert_eq!(items, ids[20..89]);
}
