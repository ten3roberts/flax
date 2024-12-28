use pretty_assertions::assert_eq;
use std::sync::Arc;

use flax::{
    components::{child_of, name},
    metadata::debuggable,
    *,
};
use glam::{vec3, Quat, Vec3};
use itertools::Itertools;
use rand::{rngs::StdRng, Rng, SeedableRng};

component! {
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
}

fn random_entities(rng: &mut impl Rng) -> impl Iterator<Item = EntityBuilder> + '_ {
    (0..).map(|_| {
        let mut builder = Entity::builder();
        if rng.gen() {
            builder.set(position(), rng.gen());
        }
        if rng.gen() {
            builder.set(rotation(), rng.gen());
        }
        if rng.gen() {
            builder.set(scale(), rng.gen());
        }

        builder
    })
}

#[test]
fn merge() {
    let mut world1 = World::new();

    let mut rng = StdRng::seed_from_u64(48);

    let placeholders = world1.spawn_many().take(10).collect_vec();

    random_entities(&mut rng)
        .take(40)
        .enumerate()
        .for_each(|(i, mut v)| {
            v.set(name(), format!("a.{i}")).spawn(&mut world1);
        });

    let mut world2 = World::new();

    random_entities(&mut rng)
        .take(40)
        .enumerate()
        .for_each(|(i, mut v)| {
            v.set(name(), format!("b.{i}")).spawn(&mut world2);
        });

    for id in placeholders {
        world1.despawn(id).unwrap();
    }

    let _new_ids = world1.merge_with(&mut world2);

    assert_eq!(Query::new(position()).borrow(&world2).count(), 0);
    assert_eq!(Query::new(name()).borrow(&world1).count(), 80);
}

#[test]
#[cfg(feature = "serde")]
/// Merge into an empty world
fn merge_empty() -> anyhow::Result<()> {
    use bincode::Options;
    use flax::{filter::All, serialize::*};
    use serde::de::DeserializeSeed;

    let mut world = World::new();

    let mut rng = StdRng::seed_from_u64(83);
    random_entities(&mut rng)
        .take(128)
        .enumerate()
        .for_each(|(i, mut v)| {
            v.set(name(), format!("world.{i}")).spawn(&mut world);
        });

    let context = SerializationContextBuilder::new()
        .with_name("position", position())
        .with_name("rotation", rotation())
        .with_name("scale", scale())
        .with_name("name", name())
        .build();

    let bytes = bincode::serialize(&context.serialize_world(&world, SerializeFormat::ColumnMajor))?;

    // Clear the world
    world.despawn_many(All);

    let mut new_world =
        context
            .deserialize_world()
            .deserialize(&mut bincode::de::Deserializer::from_slice(
                &bytes,
                bincode::DefaultOptions::new()
                    .with_fixint_encoding()
                    .allow_trailing_bytes(),
            ))?;

    assert_eq!(Query::new(()).borrow(&new_world).count(), 128);
    assert_eq!(Query::new(()).borrow(&world).count(), 0);

    let migrated = world.merge_with(&mut new_world);
    // Since the destination is empty there will be no migrated entities
    assert!(migrated.ids().is_empty());

    assert_eq!(Query::new(()).borrow(&new_world).count(), 0);
    assert_eq!(Query::new(()).borrow(&world).count(), 128);

    Ok(())
}

#[test]
fn merge_hierarchy() -> anyhow::Result<()> {
    let mut src_world = World::new();
    let mut rng = StdRng::seed_from_u64(67);

    let root = Entity::builder()
        .set(name(), "root".into())
        .set(position(), vec3(1.0, 3.0, 2.3))
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "child.1".into())
                .set(position(), vec3(1.3, 3.2, 23.0))
                .set(rotation(), Quat::from_scaled_axis(vec3(1.0, 0.0, 0.0)))
                .attach(
                    child_of,
                    Entity::builder()
                        .set(name(), "child.1.1".into())
                        .set(position(), vec3(3.5, 2.4, 3.4)),
                ),
        )
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "child.2".into())
                .set(position(), vec3(1.3, 3.2, 23.0)),
        )
        .spawn(&mut src_world);

    let mut world = World::new();

    random_entities(&mut rng)
        .take(100)
        .enumerate()
        .for_each(|(i, mut v)| {
            v.set(name(), format!("a.{i}")).spawn(&mut world);
        });

    let migrated = world.merge_with(&mut src_world);

    assert_eq!(Query::new(()).borrow(&src_world).count(), 0);
    assert_eq!(Query::new(()).borrow(&world).count(), 104);

    let new_root = migrated.get(root);

    let children = Query::new(name())
        .with(child_of(new_root))
        .borrow(&world)
        .iter()
        .cloned()
        .collect_vec();

    assert_eq!(children, ["child.1", "child.2"]);

    let child_1_1 = Query::new(position())
        .with_filter(name().eq("child.1.1".to_string()))
        .borrow(&world)
        .iter()
        .copied()
        .next();

    assert_eq!(child_1_1, Some(vec3(3.5, 2.4, 3.4)));

    dbg!(&world);

    Ok(())
}

#[test]
fn merge_custom() {
    component! {
        resources,
    }

    let mut src_world = World::new();
    src_world
        .set(resources(), name(), "resources".into())
        .unwrap();

    let custom_component =
        src_world.spawn_component(component_vtable!(custom: Arc<String> => [ Debuggable ]));

    let unused_component = src_world.spawn_component(component_vtable!(unused: f32));

    let custom_relation = src_world.spawn_relation(component_vtable!(relation: String));

    let shared: Arc<String> = Arc::new("Very important data".into());

    let mut rng = StdRng::seed_from_u64(62);

    assert!(src_world.has(custom_component.id(), debuggable()));

    let root = Entity::builder()
        .set(name(), "root".into())
        .set(position(), rng.gen())
        .set(rotation(), rng.gen())
        .attach(
            child_of,
            Entity::builder()
                .set(name(), "child.1".into())
                .set(rotation(), rng.gen()),
        )
        .set(custom_component, shared.clone())
        .attach_with(
            custom_relation,
            "Mom".into(),
            Entity::builder()
                .set(name(), "child_custom.1".into())
                .set(scale(), rng.gen()),
        )
        .spawn(&mut src_world);

    src_world
        .set(
            resources(),
            custom_component,
            Arc::new("static resource".into()),
        )
        .unwrap();

    assert!(src_world.has(custom_component.id(), debuggable()));

    let mut world = World::new();

    let custom2 =
        world.spawn_component::<Arc<String>>(flax::component_vtable!(custom2: Arc<String>));

    assert_eq!(custom2.name(), "custom2");

    world
        .set(resources(), custom2, Arc::new("String".into()))
        .unwrap();

    random_entities(&mut rng)
        .take(100)
        .enumerate()
        .for_each(|(i, mut v)| {
            v.set(name(), format!("a.{i}")).spawn(&mut world);
        });

    let migrated = world.merge_with(&mut src_world);

    let new_root = migrated.get(root);

    let new_custom_component = world
        .find_component::<Arc<String>>(migrated.get_component(custom_component).key())
        .expect("Missing component");

    let _ = migrated.get_component::<f32>(unused_component);

    assert!(world.has(new_custom_component.id(), debuggable()));

    let new_custom_relation = migrated.get_relation(custom_relation);

    assert_eq!(
        world.get(new_root, new_custom_component).as_deref(),
        Ok(&shared)
    );

    let custom_children = Query::new(name())
        .with(new_custom_relation(new_root))
        .borrow(&world)
        .iter()
        .cloned()
        .collect_vec();

    pretty_assertions::assert_eq!(
        world.get(resources(), custom2).as_deref(),
        Ok(&Arc::new("String".into()))
    );

    pretty_assertions::assert_eq!(
        world.get(resources(), new_custom_component).as_deref(),
        Ok(&Arc::new("static resource".into()))
    );

    pretty_assertions::assert_eq!(custom_children, ["child_custom.1"]);
}
