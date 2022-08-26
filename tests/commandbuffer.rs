use std::sync::Arc;

use flax::components::*;
use flax::*;

use itertools::Itertools;
component! {
    shared: Arc<String>,
    health: f32,
}

#[test]
fn commandbuffer() {
    let mut world = World::new();
    let ids = world.spawn_many().take(10).collect_vec();

    let mut cmd = CommandBuffer::new();

    let shared_value = Arc::new("Foo".to_string());

    cmd.set(ids[1], name(), "Johnathan".into())
        .set(ids[3], shared(), shared_value.clone())
        .set(ids[6], name(), "Bertha".into())
        .set(ids[8], health(), 28.5);

    cmd.apply(&mut world).unwrap();

    assert_eq!(
        world.get(ids[1], name()).as_deref(),
        Ok(&"Johnathan".to_string())
    );

    assert_eq!(world.get(ids[3], shared()).as_deref(), Ok(&shared_value));

    assert_eq!(
        world.get(ids[6], name()).as_deref(),
        Ok(&"Bertha".to_string())
    );

    assert_eq!(world.get(ids[8], health()).as_deref(), Ok(&28.5));

    // Add a name to each component which doesn't have a name
    let mut query = Query::new(entities()).filter(name().without());

    // Deferred world modification while iterating
    query.borrow(&world).iter().enumerate().for_each(|(i, id)| {
        eprintln!("Adding name to id: {id}");
        cmd.set(id, name(), format!("Unnamed: {i}"));
    });

    cmd.apply(&mut world).unwrap();

    let mut name_query = Query::new(name());
    let names = name_query
        .borrow(&world)
        .iter()
        .cloned()
        .sorted()
        .collect_vec();

    assert_eq!(
        names,
        [
            "Bertha",
            "Johnathan",
            "Unnamed: 0",
            "Unnamed: 1",
            "Unnamed: 2",
            "Unnamed: 3",
            "Unnamed: 4",
            "Unnamed: 5",
            "Unnamed: 6",
            "Unnamed: 7",
        ]
    );

    Query::new((entities(), name()))
        .filter(name().cmp(|name| name.contains("Unnamed")))
        .borrow(&world)
        .iter()
        .for_each(|(id, n)| {
            eprintln!("Removing name for entity: {id} {n}");
            cmd.remove(id, name());
        });

    cmd.despawn(ids[8]);

    cmd.apply(&mut world).unwrap();

    let names = name_query
        .borrow(&world)
        .iter()
        .cloned()
        .sorted()
        .collect_vec();

    assert_eq!(names, ["Bertha", "Johnathan"]);

    assert!(!world.is_alive(ids[8]));

    // Spawn some entities
    component! {
        soldier: (),
    }

    (0..100).for_each(|i| {
        EntityBuilder::new()
            .set_default(soldier())
            .set(name(), format!("Soldier: {i}"))
            .set(health(), 100.0)
            .spawn_into(&mut cmd)
    });

    cmd.apply(&mut world).unwrap();

    let soldiers = Query::new(health())
        .filter(soldier().with())
        .borrow(&world)
        .iter()
        .copied()
        .collect_vec();

    // Ensure all soldiers are present and up to health
    assert_eq!(soldiers, [100.0; 100]);

    // Oh no, one got shot
    if let Some(health) = Query::new(health().as_mut())
        .filter(soldier().with())
        .borrow(&world)
        .iter()
        .nth(42)
    {
        *health -= 20.0
    }

    // Well, there are only 99 unwounded soldiers left
    // Lets count them
    let soldiers = Query::new(name())
        .filter(soldier().with() & health().gte(100.0))
        .borrow(&world)
        .iter()
        .cloned()
        .collect_vec();

    assert_eq!(soldiers.len(), 99);
}
