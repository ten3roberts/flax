use flax::*;
use itertools::Itertools;
use pretty_assertions::assert_eq;
use std::sync::Arc;

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

    let mut query = Query::new(a().cloned()).filter(a().modified());

    let items = query.borrow(&world).iter().collect_vec();

    // All changed entities
    assert_eq!(items.len(), 11);

    eprintln!("Current change: {}", world.change_tick());
    world.set(id1, a(), 34.0).unwrap();

    eprintln!("Current change: {}", world.change_tick());

    let items = query.borrow(&world).iter().collect_vec();

    assert_eq!(items, &[34.0]);

    others[3..7].iter().for_each(|id| {
        let mut a = world.get_mut(*id, a()).unwrap();
        *a = -*a;
    });

    let items = query.borrow(&world).iter().collect_vec();

    eprintln!("Items: {items:?}");

    assert_eq!(items, &[-3.0, -4.0, -5.0, -6.0]);

    others[3..5].iter().for_each(|id| {
        let mut a = world.get_mut(*id, a()).unwrap();
        *a *= 10.0;
    });

    let items = query.borrow(&world).iter().collect_vec();
    assert_eq!(items, &[-30.0, -40.0]);

    // Construct a new interted query

    let mut query = Query::new(a().cloned()).filter(a().inserted());

    let items = query
        .borrow(&world)
        .iter()
        .sorted_by_key(|v| (v * 256.0) as i64)
        .collect_vec();

    assert_eq!(
        items,
        &[-40.0, -30.0, -6.0, -5.0, 0.0, 1.0, 2.0, 7.0, 8.0, 9.0, 34.0]
    );

    world.set(id2, a(), 29.5).unwrap();

    let items = query
        .borrow(&world)
        .iter()
        .sorted_by_key(|v| (v * 256.0) as i64)
        .collect_vec();

    assert_eq!(items, &[29.5]);

    let mut query = Query::new(entity_ids()).filter(a().removed());

    let items = query.borrow(&world).iter().collect_vec();

    assert_eq!(items, []);
    world.remove(id2, a()).unwrap();
    eprintln!("Removed {id2}");

    let items = query.borrow(&world).iter().collect_vec();

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

    let mut query = Query::new(entity_ids()).filter(a().modified() | b().modified());

    // eprintln!("Items: {:?}", query.iter(&world).sorted().collect_vec());
    assert_eq!(query.borrow(&world).iter().sorted().collect_vec(), ids);

    for &id in &ids[50..67] {
        *world.get_mut(id, a()).unwrap() *= -2;
    }

    let items = query.borrow(&world).iter().sorted().collect_vec();
    eprintln!("Items: {items:?}");

    assert_eq!(items, ids[50..67]);
    let items = query.borrow(&world).iter().sorted().collect_vec();
    assert_eq!(items, []);

    for &id in &ids[20..43] {
        *world.get_mut(id, a()).unwrap() *= -2;
    }

    for &id in &ids[40..89] {
        world.get_mut(id, b()).unwrap().push_str("...");
    }

    let items = query.borrow(&world).iter().sorted().collect_vec();

    assert_eq!(items, ids[20..89]);
}

#[test]
fn bitops() {
    let mut world = World::new();

    let id1 = Entity::builder()
        .set(a(), 4.5)
        .set(b(), "foo".into())
        .spawn(&mut world);

    let id2 = Entity::builder()
        .set(a(), 8.1)
        .set(d(), "bar")
        .spawn(&mut world);
    let id3 = Entity::builder()
        .set(a(), -5.1)
        .set(c(), Arc::new(5))
        .spawn(&mut world);

    assert_eq!(
        Query::new(entity_ids())
            .filter(a().gt(1.1) & a().lt(5.0))
            .borrow(&world)
            .iter()
            .collect_vec(),
        vec![id1]
    );

    assert_eq!(
        Query::new(entity_ids())
            .filter((a().gt(1.1) & a().lt(5.0)) | (d().without() & b().without()))
            .borrow(&world)
            .iter()
            .collect_vec(),
        vec![id1, id3]
    );

    assert_eq!(
        Query::new(entity_ids())
            .filter((a().cmp(|&v: &f32| v > 5.1 && v < 9.0)) | (d().without() & b().without()))
            .borrow(&world)
            .iter()
            .collect_vec(),
        vec![id2, id3]
    );
}

#[test]
fn sparse_or() {
    let mut world = World::new();

    let ids = (0..10)
        .map(|_| {
            Entity::builder()
                .set(a(), 5.4)
                .set(b(), "Foo".into())
                .spawn(&mut world)
        })
        .collect_vec();

    let mut query = Query::new(entity_ids()).filter(a().modified() | b().modified());

    assert_eq!(query.borrow(&world).iter().collect_vec(), ids);

    // ###--------
    // --###---##

    world.set(ids[0], a(), 7.1).unwrap();
    world.set(ids[1], a(), 7.1).unwrap();
    world.set(ids[2], a(), 7.1).unwrap();

    world.set(ids[2], b(), "Bar".into()).unwrap();
    world.set(ids[3], b(), "Bar".into()).unwrap();
    world.set(ids[4], b(), "Bar".into()).unwrap();
    world.set(ids[8], b(), "Bar".into()).unwrap();
    world.set(ids[9], b(), "Bar".into()).unwrap();

    {
        let mut batches = query.borrow(&world);
        let mut batches = batches.iter_batched();

        let slots = batches.map(|v| v.collect_vec()).collect_vec();
        assert_eq!(slots, &[&ids[0..=2], &ids[3..=4], &ids[8..=9]]);
        // assert_eq!(batches.next().unwrap().collect_vec(), ids[8..=9]);
        // assert!(batches.next().is_none());
    }

    // Check access compatability
    let system_a = System::builder()
        .with(query)
        .build(|_query: QueryBorrow<EntityIds, _>| {})
        .boxed();

    let system_b = System::builder()
        .with(Query::new(a().as_mut()))
        .build(|_query: QueryBorrow<_, _>| {})
        .boxed();

    let mut schedule = Schedule::from([system_a, system_b]);
    let batches = schedule.batch_info(&mut world);
    assert_eq!(batches.len(), 2);
}

#[test]
fn sparse_and() {
    let mut world = World::new();

    let ids = (0..10)
        .map(|_| {
            Entity::builder()
                .set(a(), 5.4)
                .set(b(), "Foo".into())
                .spawn(&mut world)
        })
        .collect_vec();

    let _ = (0..10)
        .map(|_| {
            Entity::builder()
                .set(a(), 5.4)
                .set(c(), Arc::new(5))
                .spawn(&mut world)
        })
        .collect_vec();

    let mut query = Query::new(entity_ids()).filter(a().modified() & b().modified());

    assert_eq!(query.borrow(&world).iter().collect_vec(), ids);

    // ###--------
    // --###---##

    world.set(ids[0], a(), 7.1).unwrap();
    world.set(ids[1], a(), 7.1).unwrap();
    world.set(ids[2], a(), 7.1).unwrap();

    world.set(ids[2], b(), "Bar".into()).unwrap();
    world.set(ids[3], b(), "Bar".into()).unwrap();
    world.set(ids[4], b(), "Bar".into()).unwrap();
    world.set(ids[8], b(), "Bar".into()).unwrap();
    world.set(ids[9], b(), "Bar".into()).unwrap();

    {
        let mut batches = query.borrow(&world);
        let mut batches = batches.iter_batched();

        assert_eq!(batches.next().unwrap().collect_vec(), ids[2..=2]);
        assert!(batches.next().is_none());
    }

    // Check access compatability
    let system_a = System::builder()
        .with(query)
        .build(|_query: QueryBorrow<EntityIds, _>| {})
        .boxed();

    let system_b = System::builder()
        .with(Query::new(a().as_mut()).with(c()))
        .build(|_query: QueryBorrow<_, _>| {})
        .boxed();

    let mut schedule = Schedule::from([system_a, system_b]);
    let batches = schedule.batch_info(&mut world);
    assert_eq!(batches.len(), 1);
}
