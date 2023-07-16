extern crate alloc;
use alloc::string::String;
use flax::{component, Entity, Error, Exclusive, World};
use std::sync::Arc;

component! {
    a: f32,
    b: String,

    relation(id): Arc<()> => [ Exclusive ],
}
#[test]
fn entity_builder() {
    let mut world = World::new();

    let id1 = Entity::builder()
        .set(a(), 1.0)
        .set(b(), "hello".into())
        .spawn(&mut world);

    let mut id2 = Entity::builder();
    id2.set(a(), 2.0).set(b(), "hello".into());
    id2.remove(b());

    let id2 = id2.spawn(&mut world);

    assert_eq!(world.get(id2, a()).as_deref(), Ok(&2.0));
    assert_eq!(
        world.get(id2, b()).as_deref(),
        Err(&Error::MissingComponent(id2, b().info()))
    );

    let value = Arc::new(());

    let mut id3 = Entity::builder();
    id3.set(a(), 2.0);
    id3.set(b(), "world".into());
    id3.set(relation(id1), value.clone());

    assert_eq!(Arc::strong_count(&value), 2);

    id3.set(relation(id1), value.clone());

    assert_eq!(Arc::strong_count(&value), 2);

    let id3 = id3.spawn(&mut world);
    assert_eq!(Arc::strong_count(&value), 2);

    world.despawn(id3).unwrap();
    assert_eq!(Arc::strong_count(&value), 1);
}
