use std::{
    any::Any, borrow::BorrowMut, collections::BTreeMap, hash::BuildHasherDefault,
    marker::PhantomData,
};

use serde::{
    de::{self, DeserializeSeed, SeqAccess, VariantAccess, Visitor},
    Deserialize, Deserializer,
};

use crate::{
    archetype::{BatchSpawn, Storage},
    util::TupleCloned,
    Archetype, Archetypes, Component, ComponentInfo, ComponentValue, Entity, EntityBuilder, World,
};

use super::{ComponentKey, SerializeFormat, WorldFields};

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    deser_col: fn(
        deserializer: &mut dyn erased_serde::Deserializer,
        len: usize,
        component: ComponentInfo,
    ) -> erased_serde::Result<Storage>,
    deser_one: fn(
        deserializer: &mut dyn erased_serde::Deserializer,
        component: ComponentInfo,
        builder: &mut EntityBuilder,
    ) -> erased_serde::Result<()>,
    info: ComponentInfo,
}

/// [ T, T, T ]
struct DeserializeStorage<'a> {
    slot: &'a Slot,
    len: usize,
}

impl<'a, 'de> DeserializeSeed<'de> for DeserializeStorage<'a> {
    type Value = Storage;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut deserializer = <dyn erased_serde::Deserializer>::erase(deserializer);
        let storage = (self.slot.deser_col)(&mut deserializer, self.len, self.slot.info)
            .map_err(de::Error::custom)?;

        Ok(storage)
    }
}

#[derive(Clone, Default)]
pub struct DeserializeBuilder {
    slots: BTreeMap<String, Slot>,
}

impl DeserializeBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with<T>(&mut self, key: impl Into<String>, component: Component<T>) -> &mut Self
    where
        T: ComponentValue + for<'x> Deserialize<'x>,
    {
        fn deser_col<T: ComponentValue + for<'x> Deserialize<'x>>(
            deserializer: &mut dyn erased_serde::Deserializer,
            len: usize,
            info: ComponentInfo,
        ) -> erased_serde::Result<Storage> {
            deserializer.deserialize_seq(StorageVisitor::<T> {
                info,
                cap: len,
                _marker: PhantomData,
            })
        }

        fn deser_one<T: ComponentValue + for<'x> Deserialize<'x>>(
            deserializer: &mut dyn erased_serde::Deserializer,
            info: ComponentInfo,
            builder: &mut EntityBuilder,
        ) -> erased_serde::Result<()> {
            let value = T::deserialize(deserializer)?;
            builder.set_dyn(info, value);
            Ok(())
        }

        let key = key.into();

        self.slots.insert(
            key,
            Slot {
                deser_col: deser_col::<T>,
                deser_one: deser_one::<T>,
                info: component.info(),
            },
        );
        self
    }

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
    pub fn deserialize<'de, D>(&self, deserializer: D) -> std::result::Result<World, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_enum("World", &["row", "col"], WorldVisitor { context: self })
    }

    fn get(&self, key: &str) -> Result<&Slot, String> {
        self.slots
            .get(key)
            .ok_or_else(|| format!("Unknown component key: {key:?}"))
    }
}

struct WorldVisitor<'a> {
    context: &'a DeserializeContext,
}

impl<'a, 'de> Visitor<'de> for WorldVisitor<'a> {
    type Value = World;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
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

impl<'de, 'a> DeserializeSeed<'de> for DeserializeEntities<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'a> Visitor<'de> for DeserializeEntities<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "an entity id followed by a map of components")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut builder = EntityBuilder::new();
        while let Some(()) = seq.next_element_seed(DeserializeEntity {
            context: self.context,
            builder: &mut builder,
        })? {
            builder.spawn(self.world);
        }

        Ok(())
    }
}

/// (id, components)
struct DeserializeEntity<'a> {
    context: &'a DeserializeContext,
    builder: &'a mut EntityBuilder,
}

impl<'de, 'a> DeserializeSeed<'de> for DeserializeEntity<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple_struct("Entity", 2, self)
    }
}

impl<'de, 'a> Visitor<'de> for DeserializeEntity<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
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

        self.builder.with_id(id);
        Ok(())
    }
}

struct DeserializeEntityData<'a> {
    context: &'a DeserializeContext,
    builder: &'a mut EntityBuilder,
}

impl<'de, 'a> DeserializeSeed<'de> for DeserializeEntityData<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

/// { component: value }
impl<'de, 'a> Visitor<'de> for DeserializeEntityData<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a map of component values")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<&str>()? {
            let slot = self.context.get(key).map_err(de::Error::custom)?;
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

impl<'de, 'a> DeserializeSeed<'de> for DeserializeComponent<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut deserializer = <dyn erased_serde::Deserializer>::erase(deserializer);
        (self.slot.deser_one)(&mut deserializer, self.slot.info, self.builder)
            .map_err(de::Error::custom)?;

        Ok(())
    }
}

/// Deserializes a list of archetypes
struct WorldRowVisitor<'a> {
    context: &'a DeserializeContext,
}

impl<'de, 'a> Visitor<'de> for WorldRowVisitor<'a> {
    type Value = World;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a struct containing a sequence of entities")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut world = World::new();
        while let Some(key) = map.next_key()? {
            match key {
                "entities" => map.next_value_seed(DeserializeEntities {
                    context: self.context,
                    world: &mut world,
                })?,
                key => return Err(de::Error::unknown_field(key, &["entities"])),
            }
        }

        Ok(world)
    }
}

/// Deserializes a list of archetypes
struct WorldColumnVisitor<'a> {
    context: &'a DeserializeContext,
}

impl<'de, 'a> Visitor<'de> for WorldColumnVisitor<'a> {
    type Value = World;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
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

impl<'a, 'de> DeserializeSeed<'de> for DeserializeArchetypes<'a> {
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

impl<'a, 'de> Visitor<'de> for ArchetypesVisitor<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
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

impl<'de, 'a> DeserializeSeed<'de> for DeserializeArchetype<'a> {
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

impl<'a, 'de> Visitor<'de> for ArchetypeVisitor<'a> {
    type Value = (Vec<Entity>, BatchSpawn);

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
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

impl<'de, 'a> DeserializeSeed<'de> for DeserializeStorages<'a> {
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

impl<'de, 'a> Visitor<'de> for StoragesVisitor<'a> {
    type Value = BatchSpawn;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a map of component values")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut batch = BatchSpawn::new(self.len);
        while let Some(key) = map.next_key::<&'de str>()? {
            let slot = self.context.get(key).map_err(de::Error::custom)?;

            let storage = map.next_value_seed(DeserializeStorage {
                slot,
                len: self.len,
            })?;

            batch.insert(storage).map_err(de::Error::custom)?;
        }

        Ok(batch)
    }
}

/// Visit a single column of component values
struct StorageVisitor<T: ComponentValue> {
    info: ComponentInfo,
    cap: usize,
    _marker: PhantomData<T>,
}

impl<'de, T: ComponentValue + de::Deserialize<'de>> Visitor<'de> for StorageVisitor<T> {
    type Value = Storage;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "A sequence of component values of the same type")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut storage = Storage::with_capacity(self.info, self.cap);

        while let Some(item) = seq.next_element::<T>()? {
            storage.push(item)
        }

        Ok(storage)
    }
}
