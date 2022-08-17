mod deserialize;
mod serialize;

pub use deserialize::*;
use serde::{Deserialize, Serialize};
pub use serialize::*;

use crate::{And, Component, ComponentId, ComponentValue, StaticFilter, Without};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ComponentKey {
    key: String,
    id: ComponentId,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum WorldFields {
    #[serde(rename = "archetypes")]
    Archetypes,
}

/// Describes the serialialization format
#[derive(Debug, Clone, serde::Deserialize)]
pub enum SerializeFormat {
    /// Serialize the world in a column major format.
    /// This is more efficient but less human readable.
    #[serde(rename = "col")]
    ColumnMajor,
    /// Serialize the world in a row major format.
    /// This is less efficient and uses slightly more space since each entity is
    /// serialized as a map, though it is more human readable and easier for git
    /// merges.
    #[serde(rename = "row")]
    RowMajor,
}

/// Allows constructing a serialize and deserialize context with the same
/// supported types allowing for easier roundtrips.
pub struct SerdeBuilder<F> {
    ser: SerializeBuilder<F>,
    de: DeserializeBuilder,
}

impl SerdeBuilder<Without> {
    /// Creates a new builder which simultaneously constructs a serialialization
    /// and deserialization context
    pub fn new() -> Self {
        Self {
            ser: Default::default(),
            de: Default::default(),
        }
    }
}

impl Default for SerdeBuilder<Without> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F> SerdeBuilder<F>
where
    F: StaticFilter + 'static + Clone,
{
    /// Register a component for both serialization and deserialiaztion
    pub fn with<T: ComponentValue + Serialize + for<'de> Deserialize<'de>>(
        &mut self,
        key: impl Into<String>,
        component: Component<T>,
    ) -> &mut Self {
        let key = key.into();
        self.ser.with(key.clone(), component);
        self.de.with(key, component);
        self
    }

    /// Add a new filter to specify which entities will be serialized.
    pub fn with_filter<G>(self, filter: G) -> SerdeBuilder<And<F, G>> {
        SerdeBuilder {
            ser: self.ser.with_filter(filter),
            de: self.de,
        }
    }

    /// Finish constructing the serialize and deserialize context.
    pub fn build(&mut self) -> (SerializeContext, DeserializeContext) {
        (self.ser.build(), self.de.build())
    }
}

#[cfg(test)]
mod test {
    use rand::{
        distributions::{Standard, Uniform},
        rngs::StdRng,
        Rng, SeedableRng,
    };

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
            .set(name(), "Player".to_string())
            .set(pos(), (1.4, 5.3))
            .set(items(), vec!["Dagger".to_string(), "Estradiol".to_string()])
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
            .push("Poison".to_string());

        world
            .entry(enemies[5], status_effects())
            .unwrap()
            .or_default()
            .push("Fire".to_string());

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

        let (serializer, deserializer) = SerdeBuilder::new()
            .with("name", name())
            .with("health", health())
            .with("pos", pos())
            .with("items", items())
            .with("status_effects", status_effects())
            .build();

        let json = serde_json::to_string_pretty(
            &serializer.serialize(&world, SerializeFormat::ColumnMajor),
        )
        .unwrap();
        eprintln!("World: {json}");

        let new_world: World = deserializer
            .deserialize(&mut serde_json::Deserializer::from_str(&json[..]))
            .expect("Failed to deserialize world");

        test_eq(&world, &new_world);

        let json =
            serde_json::to_string_pretty(&serializer.serialize(&world, SerializeFormat::RowMajor))
                .unwrap();
        eprintln!("World: {json}");

        let world = new_world;
        let new_world = deserializer
            .deserialize(&mut serde_json::Deserializer::from_str(&json[..]))
            .expect("Failed to deserialize world");

        test_eq(&world, &new_world)
    }
}
