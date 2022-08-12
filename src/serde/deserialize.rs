use std::{collections::BTreeMap, marker::PhantomData};

use serde::{
    de::{self, DeserializeSeed, SeqAccess, Visitor},
    Deserialize, Deserializer,
};

use crate::{
    archetype::{ComponentBatch, Storage},
    util::TupleCloned,
    Archetype, Archetypes, Component, ComponentInfo, ComponentValue, Entity, World,
};

use super::{ComponentKey, WorldFields};

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    deser_col: fn(
        deserializer: &mut dyn erased_serde::Deserializer,
        len: usize,
        component: ComponentInfo,
    ) -> erased_serde::Result<Storage>,
    info: ComponentInfo,
}

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
            .map_err(|err| de::Error::custom(err))?;

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

        let key = key.into();

        self.slots.insert(
            key,
            Slot {
                deser_col: deser_col::<T>,
                info: component.info(),
            },
        );
        self
    }
}

pub struct ColumnDeserialize {
    slots: BTreeMap<String, Slot>,
}

impl ColumnDeserialize {
    /// Deserializes the world from the supplied deserializer.
    pub fn deserialize<'de, D>(&self, deserializer: D) -> std::result::Result<World, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_struct("World", &["archetypes"], WorldVisitor { context: self })
    }
}

struct WorldVisitor<'a> {
    context: &'a ColumnDeserialize,
}

impl<'a, 'de> Visitor<'de> for WorldVisitor<'a> {
    type Value = World;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "A map like structure containing the world")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
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

        if !has_archetypes {
            return Err(de::Error::missing_field("archetypes"));
        }

        Ok(world)
    }
}

/// Deserializes a list of archetypes
struct DeserializeArchetypes<'a> {
    context: &'a ColumnDeserialize,
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
    context: &'a ColumnDeserialize,
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
        while let Some((ids, batch)) = seq.next_element_seed(DeserializeArchetype {
            context: self.context,
        })? {
            world
                .spawn_batch_at(&ids, batch)
                .expect("Entity ids are not duplicated");
        }

        Ok(())
    }
}

struct DeserializeArchetype<'a> {
    context: &'a ColumnDeserialize,
}

impl<'de, 'a> DeserializeSeed<'de> for DeserializeArchetype<'a> {
    type Value = (Vec<Entity>, ComponentBatch);

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
    context: &'a ColumnDeserialize,
}

impl<'a, 'de> Visitor<'de> for ArchetypeVisitor<'a> {
    type Value = (Vec<Entity>, ComponentBatch);

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
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;

        Ok((entities, components))
    }
}

struct DeserializeStorages<'a> {
    len: usize,
    context: &'a ColumnDeserialize,
}

impl<'de, 'a> DeserializeSeed<'de> for DeserializeStorages<'a> {
    type Value = ComponentBatch;

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
    context: &'a ColumnDeserialize,
}

impl<'de, 'a> Visitor<'de> for StoragesVisitor<'a> {
    type Value = ComponentBatch;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a map of component values")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut batch = ComponentBatch::new(self.len);
        while let Some(key) = map.next_key::<&'de str>()? {
            let slot = self
                .context
                .slots
                .get(key)
                .ok_or_else(|| de::Error::custom(format!("Unknown component key: {key:?}")))?;

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
