use core::marker::PhantomData;
use std::borrow::Cow;

use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
use serde::{
    de::{self, DeserializeSeed, SeqAccess, VariantAccess, Visitor},
    Deserialize, Deserializer,
};

use crate::{
    archetype::{ArchetypeStorage, BatchSpawn},
    component::{ComponentDesc, ComponentValue},
    Component, Entity, EntityBuilder, World,
};

use super::{
    registry::{deser_col, deser_one, REGISTRY},
    DeserializeColFn, DeserializeRowFn, RowFields, SerializeFormat, WorldFields,
};

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    deser_col: DeserializeColFn,
    deser_one: DeserializeRowFn,
    desc: ComponentDesc,
}

/// [ T, T, T ]
struct DeserializeStorage<'a> {
    slot: &'a Slot,
    len: usize,
}

impl<'de> DeserializeSeed<'de> for DeserializeStorage<'_> {
    type Value = ArchetypeStorage;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut deserializer = <dyn erased_serde::Deserializer>::erase(deserializer);
        let storage = (self.slot.deser_col)(&mut deserializer, self.slot.desc, self.len)
            .map_err(de::Error::custom)?;

        Ok(storage)
    }
}

#[derive(Clone, Default)]
/// Incrementally construct a [crate::serialize::DeserializeContext]
pub struct DeserializeBuilder {
    slots: BTreeMap<String, Slot>,
}

impl DeserializeBuilder {
    /// Creates a new [`DeserializeBuilder`]
    pub fn new() -> Self {
        Default::default()
    }

    /// Creates a component deserializer from the global registry
    pub fn from_registry(&mut self) -> &mut Self {
        self.slots.extend(REGISTRY.deserializers().iter().map(|v| {
            let desc = (v.desc)();
            (
                desc.name().to_string(),
                Slot {
                    deser_col: v.deserialize_col_fn,
                    deser_one: v.deserialize_row_fn,
                    desc,
                },
            )
        }));

        self
    }

    /// Register a component using the component's name
    ///
    /// See [`Self::with_name`]
    pub fn with<T>(&mut self, component: Component<T>) -> &mut Self
    where
        T: ComponentValue + for<'x> Deserialize<'x>,
    {
        self.with_name(component.name(), component)
    }

    /// Register a new component to be deserialized
    pub fn with_name<T>(&mut self, key: impl Into<String>, component: Component<T>) -> &mut Self
    where
        T: ComponentValue + for<'x> Deserialize<'x>,
    {
        let key = key.into();

        self.slots.insert(
            key,
            Slot {
                deser_col: deser_col::<T>,
                deser_one: deser_one::<T>,
                desc: component.desc(),
            },
        );
        self
    }

    /// Finish constructing the deserialization context
    pub fn build(&mut self) -> DeserializeContext {
        DeserializeContext {
            slots: self.slots.clone(),
        }
    }
}

/// Describes how to deserialize the world from the described components.
pub struct DeserializeContext {
    slots: BTreeMap<String, Slot>,
}

impl DeserializeContext {
    /// Deserializes the world from the supplied deserializer.
    /// Automatically uses the row or column major format depending on the
    /// underlying data.
    pub fn deserialize_world(&self) -> WorldDeserializer {
        WorldDeserializer { context: self }
    }

    /// Deserializes an entity into the provided builder
    pub fn deserialize_entity(&self) -> EntityDataDeserializer {
        EntityDataDeserializer { context: self }
    }

    fn get(&self, key: &str) -> Result<&Slot, String> {
        self.slots
            .get(key)
            .ok_or_else(|| format!("Unknown component key: {key:?}"))
    }
}

/// Deserializes an entire world in either column or row format
pub struct WorldDeserializer<'a> {
    context: &'a DeserializeContext,
}

impl<'de> DeserializeSeed<'de> for WorldDeserializer<'_> {
    type Value = World;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_enum(
            "World",
            &["row", "col"],
            WorldFormatVisitor {
                context: self.context,
            },
        )
    }
}

/// Deserializes a single entity (map-like format)
pub struct EntityDataDeserializer<'a> {
    context: &'a DeserializeContext,
}

impl<'de> DeserializeSeed<'de> for EntityDataDeserializer<'_> {
    type Value = EntityBuilder;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for EntityDataDeserializer<'_> {
    type Value = EntityBuilder;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "a map of component values")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut builder = EntityBuilder::new();
        while let Some(key) = map.next_key::<Cow<'de, str>>().unwrap() {
            let slot = self.context.get(&key).map_err(de::Error::custom)?;
            map.next_value_seed(DeserializeComponent {
                slot,
                builder: &mut builder,
            })?;
        }

        Ok(builder)
    }
}

struct WorldFormatVisitor<'a> {
    context: &'a DeserializeContext,
}

impl<'de> Visitor<'de> for WorldFormatVisitor<'_> {
    type Value = World;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "A map like structure containing the world")
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: de::EnumAccess<'de>,
    {
        let (format, variant) = data.variant::<SerializeFormat>()?;
        let world = match format {
            SerializeFormat::ColumnMajor => variant.struct_variant(
                &["archetypes"],
                WorldColumnVisitor {
                    context: self.context,
                },
            )?,
            SerializeFormat::RowMajor => variant.struct_variant(
                &["entities"],
                WorldRowVisitor {
                    context: self.context,
                },
            )?,
        };
        Ok(world)
    }
}

struct DeserializeEntities<'a> {
    context: &'a DeserializeContext,
    world: &'a mut World,
}

impl<'de> DeserializeSeed<'de> for DeserializeEntities<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for DeserializeEntities<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "an entity id followed by a map of components")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut builder = EntityBuilder::new();
        while let Some(id) = seq.next_element_seed(EntityVisitor {
            context: self.context,
            builder: &mut builder,
        })? {
            // The world that is serialized into is empty
            builder.spawn_at(self.world, id).map_err(|e| {
                de::Error::custom(format!("Duplicate entities in deserialized world: {e}"))
            })?;
        }

        Ok(())
    }
}

/// (id, components)
struct EntityVisitor<'a> {
    context: &'a DeserializeContext,
    builder: &'a mut EntityBuilder,
}

impl<'de> DeserializeSeed<'de> for EntityVisitor<'_> {
    type Value = Entity;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple_struct("Entity", 2, self)
    }
}

impl<'de> Visitor<'de> for EntityVisitor<'_> {
    type Value = Entity;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "an entity id followed by a map of components")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let id = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;

        seq.next_element_seed(DeserializeEntityData {
            context: self.context,
            builder: self.builder,
        })?
        .ok_or_else(|| de::Error::invalid_length(0, &self))?;

        Ok(id)
    }
}

/// Deserialize the entity data into the provided entity builder
struct DeserializeEntityData<'a> {
    context: &'a DeserializeContext,
    builder: &'a mut EntityBuilder,
}

impl<'de> DeserializeSeed<'de> for DeserializeEntityData<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

/// { component: value }
impl<'de> Visitor<'de> for DeserializeEntityData<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "a map of component values")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            let slot = self.context.get(&key).map_err(de::Error::custom)?;
            map.next_value_seed(DeserializeComponent {
                slot,
                builder: self.builder,
            })?;
        }

        Ok(())
    }
}

/// A single component value
struct DeserializeComponent<'a> {
    slot: &'a Slot,
    builder: &'a mut EntityBuilder,
}

impl<'de> DeserializeSeed<'de> for DeserializeComponent<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut deserializer = <dyn erased_serde::Deserializer>::erase(deserializer);
        (self.slot.deser_one)(&mut deserializer, self.slot.desc, self.builder)
            .map_err(de::Error::custom)?;

        Ok(())
    }
}

/// Deserializes a list of archetypes
struct WorldRowVisitor<'a> {
    context: &'a DeserializeContext,
}

impl<'de> Visitor<'de> for WorldRowVisitor<'_> {
    type Value = World;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "a struct containing a sequence of entities")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut world = World::new();

        seq.next_element_seed(DeserializeEntities {
            context: self.context,
            world: &mut world,
        })?
        .ok_or_else(|| de::Error::invalid_length(1, &self))?;

        Ok(world)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut world = World::new();

        while let Some(key) = map.next_key()? {
            match key {
                RowFields::Entities => map.next_value_seed(DeserializeEntities {
                    context: self.context,
                    world: &mut world,
                })?,
            }
        }

        Ok(world)
    }
}

/// Deserializes a list of archetypes
struct WorldColumnVisitor<'a> {
    context: &'a DeserializeContext,
}

impl<'de> Visitor<'de> for WorldColumnVisitor<'_> {
    type Value = World;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "a struct containing a sequence of archetypes")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut world = World::new();
        let mut has_archetypes = false;

        while let Some(key) = map.next_key()? {
            match key {
                WorldFields::Archetypes => {
                    if has_archetypes {
                        return Err(de::Error::duplicate_field("archetypes"));
                    }

                    map.next_value_seed(DeserializeArchetypes {
                        context: self.context,
                        world: &mut world,
                    })?;

                    has_archetypes = true;
                }
            }
        }

        Ok(world)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut world = World::new();

        seq.next_element_seed(DeserializeArchetypes {
            context: self.context,
            world: &mut world,
        })?
        .ok_or_else(|| de::Error::invalid_length(0, &self))?;

        Ok(world)
    }
}

/// Deserializes a list of archetypes
struct DeserializeArchetypes<'a> {
    context: &'a DeserializeContext,
    world: &'a mut World,
}

impl<'de> DeserializeSeed<'de> for DeserializeArchetypes<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(ArchetypesVisitor {
            context: self.context,
            world: self.world,
        })
    }
}

struct ArchetypesVisitor<'a> {
    context: &'a DeserializeContext,
    world: &'a mut World,
}

impl<'de> Visitor<'de> for ArchetypesVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "expected a sequence of archetypes")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let world = self.world;
        while let Some((ids, mut batch)) = seq.next_element_seed(DeserializeArchetype {
            context: self.context,
        })? {
            world
                .spawn_batch_at(&ids, &mut batch)
                .expect("Entity ids are not duplicated");
        }

        Ok(())
    }
}

struct DeserializeArchetype<'a> {
    context: &'a DeserializeContext,
}

impl<'de> DeserializeSeed<'de> for DeserializeArchetype<'_> {
    type Value = (Vec<Entity>, BatchSpawn);

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple_struct(
            "Arch",
            2,
            ArchetypeVisitor {
                context: self.context,
            },
        )
    }
}

struct ArchetypeVisitor<'a> {
    context: &'a DeserializeContext,
}

impl<'de> Visitor<'de> for ArchetypeVisitor<'_> {
    type Value = (Vec<Entity>, BatchSpawn);

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "an archetype of entities and components")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let entities: Vec<Entity> = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;

        let components = seq
            .next_element_seed(DeserializeStorages {
                len: entities.len(),
                context: self.context,
            })?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;

        Ok((entities, components))
    }
}

struct DeserializeStorages<'a> {
    len: usize,
    context: &'a DeserializeContext,
}

impl<'de> DeserializeSeed<'de> for DeserializeStorages<'_> {
    type Value = BatchSpawn;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(StoragesVisitor {
            len: self.len,
            context: self.context,
        })
    }
}

struct StoragesVisitor<'a> {
    len: usize,
    context: &'a DeserializeContext,
}

impl<'de> Visitor<'de> for StoragesVisitor<'_> {
    type Value = BatchSpawn;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "a map of component values")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut batch = BatchSpawn::new(self.len);
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            let slot = self.context.get(&key).map_err(de::Error::custom)?;

            let storage = map.next_value_seed(DeserializeStorage {
                slot,
                len: self.len,
            })?;

            batch.append(storage).map_err(de::Error::custom)?;
        }

        Ok(batch)
    }
}

/// Visit a single column of component values
pub(super) struct StorageVisitor<T: ComponentValue> {
    pub(super) desc: ComponentDesc,
    pub(super) cap: usize,
    pub(super) _marker: PhantomData<T>,
}

impl<'de, T: ComponentValue + de::Deserialize<'de>> Visitor<'de> for StorageVisitor<T> {
    type Value = ArchetypeStorage;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "A sequence of component values of the same type")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut storage = ArchetypeStorage::with_capacity(self.desc, self.cap);

        while let Some(item) = seq.next_element::<T>()? {
            unsafe { storage.push(item) }
        }

        Ok(storage)
    }
}
