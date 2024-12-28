#[macro_use]
/// Global component serialization registry
pub mod registry;

mod de;
mod ser;

use alloc::string::String;
pub use de::*;
pub use ser::*;
use serde::{Deserialize, Serialize};

use crate::{
    archetype::ArchetypeStorage,
    component::{ComponentDesc, ComponentKey, ComponentValue},
    filter::{All, StaticFilter},
    Component, EntityBuilder, EntityRef, World,
};

type DeserializeRowFn = fn(
    &mut dyn erased_serde::Deserializer,
    ComponentDesc,
    &mut EntityBuilder,
) -> erased_serde::Result<()>;

type DeserializeColFn = fn(
    &mut dyn erased_serde::Deserializer,
    ComponentDesc,
    usize,
) -> erased_serde::Result<ArchetypeStorage>;

type SerializeFn = for<'x> fn(&'x ArchetypeStorage, slot: usize) -> &'x dyn erased_serde::Serialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ComponentSerKey {
    key: String,
    id: ComponentKey,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum WorldFields {
    Archetypes,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum RowFields {
    Entities,
}

/// Describes the serialialization format
#[derive(Debug, Clone, serde::Deserialize)]
pub enum SerializeFormat {
    /// Serialize the world in a row major format.
    /// This is less efficient and uses slightly more space since each entity is
    /// serialized as a map, though it is more human readable and easier for git
    /// merges.
    #[serde(rename = "row")]
    RowMajor,
    /// Serialize the world in a column major format.
    /// This is more efficient but less human readable.
    #[serde(rename = "col")]
    ColumnMajor,
}

/// Allows constructing a serialize and deserialize context with the same
/// supported types allowing for easier roundtrips.
pub struct SerializationContextBuilder {
    ser: SerializeBuilder,
    de: DeserializeBuilder,
}

impl SerializationContextBuilder {
    /// Creates a new builder which simultaneously constructs a serialialization
    /// and deserialization context
    pub fn new() -> Self {
        Self {
            ser: Default::default(),
            de: Default::default(),
        }
    }
}

impl Default for SerializationContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SerializationContextBuilder {
    /// Create the context from the global registry
    pub fn from_registry(&mut self) -> &mut Self {
        self.ser.from_registry();
        self.de.from_registry();
        self
    }

    /// Register a component using the component name.
    ///
    /// See [`Self::with_name`]
    pub fn with<T>(&mut self, component: Component<T>) -> &mut Self
    where
        T: ComponentValue + Serialize + for<'de> Deserialize<'de>,
    {
        self.with_name(component.name(), component)
    }

    /// Register a component for both serialization and deserialiaztion
    pub fn with_name<T>(&mut self, key: impl Into<String>, component: Component<T>) -> &mut Self
    where
        T: ComponentValue + Serialize + for<'de> Deserialize<'de>,
    {
        let key = key.into();
        self.ser.with_name(key.clone(), component);
        self.de.with_name(key, component);
        self
    }

    /// Finish constructing the serialize and deserialize context.
    pub fn build(&mut self) -> SerializationContext {
        SerializationContext {
            serializer: self.ser.build(),
            deserializer: self.de.build(),
        }
    }
}

/// Joint context for serialization and deserialization
pub struct SerializationContext {
    serializer: SerializeContext,
    deserializer: DeserializeContext,
}

impl SerializationContext {
    /// Creates a new [`SerializationContextBuilder`]
    pub fn builder() -> SerializationContextBuilder {
        SerializationContextBuilder::new()
    }

    /// Serialize a world
    pub fn serialize_world<'a>(
        &'a self,
        world: &'a World,
        format: SerializeFormat,
    ) -> WorldSerializer<'a, All> {
        self.serializer.serialize_world(world, format, All)
    }

    /// Serialize a world with a custom filter
    pub fn serialize_world_with_filter<'a, F: StaticFilter>(
        &'a self,
        world: &'a World,
        format: SerializeFormat,
        filter: F,
    ) -> WorldSerializer<'a, F> {
        self.serializer.serialize_world(world, format, filter)
    }

    /// Serialize a single entity's data
    pub fn serialize_entity<'a>(&'a self, entity: &EntityRef<'a>) -> EntityDataSerializer<'a> {
        self.serializer.serialize_entity(entity)
    }

    /// Automatically uses the row or column major format depending on the
    /// underlying data.
    pub fn deserialize_world(&self) -> WorldDeserializer {
        self.deserializer.deserialize_world()
    }

    /// Deserialize a single entity's data
    pub fn deserialize_entity(&self) -> EntityDataDeserializer {
        self.deserializer.deserialize_entity()
    }

    /// Returns the serialization context
    pub fn serializer(&self) -> &SerializeContext {
        &self.serializer
    }

    /// Returns the deserialization context
    pub fn deserializer(&self) -> &DeserializeContext {
        &self.deserializer
    }
}

#[cfg(test)]
mod test {
    use alloc::format;
    use alloc::vec;
    use alloc::vec::Vec;
    use rand::{
        distributions::{Standard, Uniform},
        rngs::StdRng,
        Rng, SeedableRng,
    };
    use serde::de::DeserializeSeed;

    use crate::{archetype::BatchSpawn, component, components::name, Entity, World};

    use super::*;

    #[test]
    fn serialize() {
        let mut world = World::new();

        component! {
            health: f32,
            pos: (f32, f32),
            items: Vec<String>,
            status_effects: Vec<String>,
        }

        let player = Entity::builder()
            .set(name(), "Player".into())
            .set(pos(), (1.4, 5.3))
            .set(items(), vec!["Dagger".into(), "Estradiol".into()])
            .set(health(), 67.8)
            .spawn(&mut world);

        let mut rng = StdRng::seed_from_u64(42);

        let mut batch = BatchSpawn::new(16);
        batch
            .set(name(), (0..).map(|i| format!("Enemy.{i}")))
            .unwrap();
        batch
            .set(
                health(),
                (&mut rng)
                    .sample_iter(Uniform::new(0.0, 100.0))
                    .map(|v: f32| (v * 5.0).round() / 5.0),
            )
            .unwrap();

        batch.set(pos(), (&mut rng).sample_iter(Standard)).unwrap();

        let enemies = batch.spawn(&mut world);

        world
            .entry(enemies[2], status_effects())
            .unwrap()
            .or_default()
            .push("Poison".into());

        world
            .entry(enemies[5], status_effects())
            .unwrap()
            .or_default()
            .push("Fire".into());

        let all_entities = [vec![player], enemies].concat();

        let test_eq = |world: &World, new_world: &World| {
            // Check that all are identical
            for &id in &all_entities {
                assert_eq!(
                    world.get(id, health()).as_deref(),
                    new_world.get(id, health()).as_deref()
                );

                assert_eq!(
                    world.get(id, pos()).as_deref(),
                    new_world.get(id, pos()).as_deref()
                );

                assert_eq!(
                    world.get(id, items()).as_deref(),
                    new_world.get(id, items()).as_deref()
                );

                assert_eq!(
                    world.get(id, status_effects()).as_deref(),
                    new_world.get(id, status_effects()).as_deref()
                );
            }
        };

        let context = SerializationContextBuilder::new()
            .with(name())
            .with(health())
            .with(pos())
            .with(items())
            .with(status_effects())
            .build();

        let json =
            serde_json::to_string(&context.serialize_world(&world, SerializeFormat::ColumnMajor))
                .unwrap();

        let new_world: World = context
            .deserialize_world()
            .deserialize(&mut serde_json::Deserializer::from_str(&json[..]))
            .expect("Failed to deserialize world");

        test_eq(&world, &new_world);

        let json = serde_json::to_string_pretty(
            &context.serialize_world(&world, SerializeFormat::RowMajor),
        )
        .unwrap();

        let world = new_world;
        let new_world = context
            .deserialize_world()
            .deserialize(&mut serde_json::Deserializer::from_str(&json[..]))
            .expect("Failed to deserialize world");

        test_eq(&world, &new_world);

        let encoded =
            ron::to_string(&context.serialize_world(&world, SerializeFormat::RowMajor)).unwrap();

        let new_world = context
            .deserialize_world()
            .deserialize(&mut ron::Deserializer::from_str(&encoded).unwrap())
            .unwrap();

        test_eq(&world, &new_world);

        let encoded =
            ron::to_string(&context.serialize_world(&world, SerializeFormat::ColumnMajor)).unwrap();

        let new_world = context
            .deserialize_world()
            .deserialize(&mut ron::Deserializer::from_str(&encoded).unwrap())
            .unwrap();

        test_eq(&world, &new_world);
    }
}
