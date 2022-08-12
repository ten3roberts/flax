use std::{collections::BTreeMap, mem};

use crate::{
    archetype::StorageBorrowDyn, serde::ComponentKey, Archetype, Archetypes, Component,
    ComponentId, ComponentValue, StaticFilter, World,
};

use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, SerializeTupleStruct};

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    ser_col: for<'a> fn(storage: &'a StorageBorrowDyn) -> Box<dyn erased_serde::Serialize>,
    key: ComponentKey,
}

#[derive(Clone, Default)]
pub struct SerializeBuilder {
    serializers: BTreeMap<ComponentId, Slot>,
}

impl SerializeBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with<T: ComponentValue + serde::Serialize>(
        &mut self,
        key: impl Into<String>,
        component: Component<T>,
    ) -> &mut Self {
        fn ser_col<'a, T: serde::Serialize + ComponentValue>(
            storage: &'a StorageBorrowDyn<'_>,
        ) -> Box<dyn erased_serde::Serialize> {
            let data = unsafe { storage.as_slice::<T>() };
            Box::new(data)
        }

        let key = key.into();

        self.serializers.insert(
            component.id(),
            Slot {
                key: ComponentKey::new(key, component.id()),
                ser_col: ser_col::<T>,
            },
        );

        self
    }

    pub fn build(&mut self) -> ColumnSerialize {
        ColumnSerialize {
            serializers: mem::take(&mut self.serializers),
        }
    }
}

/// Describes how to serialize a world into columns.
pub struct ColumnSerialize {
    serializers: BTreeMap<ComponentId, Slot>,
}

impl ColumnSerialize {
    pub fn builder() -> SerializeBuilder {
        SerializeBuilder::new()
    }
}

struct WorldSerializer<'a, F> {
    world: &'a World,
    context: &'a ColumnSerialize,
    filter: F,
}

impl<'a, F> serde::Serialize for WorldSerializer<'a, F>
where
    F: StaticFilter,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("World", 3)?;
        state.serialize_field(
            "archetypes",
            &SerializeArchetypes {
                archetypes: &self.world.archetypes,
                filter: &self.filter,
                context: self.context,
            },
        )?;

        state.end()
    }
}

struct SerializeArchetypes<'a, F> {
    archetypes: &'a Archetypes,
    filter: &'a F,
    context: &'a ColumnSerialize,
}

impl<'a, F> serde::Serialize for SerializeArchetypes<'a, F>
where
    F: StaticFilter,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_seq(Some(self.archetypes.len()))?;

        for (_, arch) in self.archetypes.iter() {
            if self.filter.static_matches(arch) {
                state.serialize_element(&SerializeStorage {
                    storage: arch,
                    context: self.context,
                })?;
            }
        }

        state.end()
    }
}

struct SerializeArchetype<'a> {
    arch: &'a Archetype,
    context: &'a ColumnSerialize,
}

struct SerializeStorage<'a> {
    storage: &'a Archetype,
    context: &'a ColumnSerialize,
}

impl<'a> serde::Serialize for SerializeStorage<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_map(None)?;

        for storage in self.storage.storages() {
            let id = storage.info().id;
            if let Some(slot) = self.context.serializers.get(&id) {
                state.serialize_entry(&slot.key, (slot.ser_col)(&storage))?;
            }
        }

        state.end()
    }
}

impl<'a> serde::Serialize for SerializeArchetype<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_tuple_struct("Arch", 3)?;
        state.serialize_field(self.arch.entities())?;
        state.serialize_field(&SerializeStorage {
            storage: self.arch,
            context: self.context,
        })?;

        state.end()
    }
}
