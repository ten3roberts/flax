use std::sync::Arc;

use flax::*;
use itertools::Itertools;
component! {
    name: String,
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

    let mut query = Query::new(entities()).filter(name().without());

    // Deferred world modification while iterating
    query
        .prepare(&world)
        .iter()
        .enumerate()
        .for_each(|(i, id)| {
            eprintln!("Adding name to id: {id}");
            cmd.set(id, name(), format!("Unnamed: {i}"));
        });

    // Yes, you can also name components.
    // How nifty
    cmd.set(shared(), name(), "Shared State".into());
    cmd.set(name(), name(), "Entity Name".into());
    cmd.apply(&mut world).unwrap();

    let names = Query::new(name())
        .prepare(&world)
        .iter()
        .cloned()
        .sorted()
        .collect_vec();

    assert_eq!(
        names,
        [
            "Bertha",
            "Entity Name",
            "Johnathan",
            "Shared State",
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
}
