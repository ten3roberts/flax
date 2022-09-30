use crate::{
    archetype::StorageBorrowDyn, components::is_component, filter::And, filter::StaticFilter,
    filter::Without, Archetype, ArchetypeId, Component, ComponentKey, ComponentValue, Entity,
    World,
};

use alloc::{boxed::Box, collections::BTreeMap, string::String};
use serde::{
    ser::{SerializeMap, SerializeSeq, SerializeStructVariant, SerializeTupleStruct},
    Serialize, Serializer,
};

use super::SerializeFormat;

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    ser: for<'x> fn(
        storage: &'x StorageBorrowDyn<'_>,
        slot: usize,
    ) -> &'x dyn erased_serde::Serialize,
    key: String,
}

#[derive(Clone)]
/// Builder for a serialialization context
pub struct SerializeBuilder<F> {
    slots: BTreeMap<ComponentKey, Slot>,
    filter: F,
}

impl SerializeBuilder<Without> {
    /// Creates a new SerializeBuilder
    pub fn new() -> Self {
        Self {
            slots: Default::default(),
            filter: is_component().without(),
        }
    }
}

impl Default for SerializeBuilder<Without> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F> SerializeBuilder<F>
where
    F: StaticFilter + 'static + Clone,
{
    /// Register a component using the component name.
    ///
    /// See [`Self::with_name`]
    pub fn with<T>(&mut self, component: Component<T>) -> &mut Self
    where
        T: ComponentValue + Serialize,
    {
        self.with_name(component.name(), component)
    }

    /// Register a new component to be serialized if encountered.
    /// And entity will still be serialized if it only contains a non-empty
    /// subset of the registered components.
    pub fn with_name<T>(&mut self, key: impl Into<String>, component: Component<T>) -> &mut Self
    where
        T: ComponentValue + serde::Serialize,
    {
        fn ser_col<'a, T: serde::Serialize + ComponentValue>(
            storage: &'a StorageBorrowDyn<'_>,
            slot: usize,
        ) -> &'a dyn erased_serde::Serialize {
            let ptr = storage.at(slot).expect("Slot outside range");
            unsafe {
                let val = ptr.cast::<T>().as_ref().expect("not null");
                val
            }
        }

        self.slots.insert(
            component.key(),
            Slot {
                key: key.into(),
                ser: ser_col::<T>,
            },
        );

        self
    }

    /// Add a new filter to specify which entities will be serialized.
    pub fn with_filter<G>(self, filter: G) -> SerializeBuilder<And<F, G>> {
        SerializeBuilder {
            slots: self.slots,
            filter: And::new(self.filter, filter),
        }
    }

    /// Finish constructing the serialization context
    pub fn build(&mut self) -> SerializeContext {
        SerializeContext {
            slots: self.slots.clone(),
            filter: Box::new(self.filter.clone()),
        }
    }
}

/// Describes how to serialize a world given a group of components to serialize
/// and an optional filter. Empty entities will be skipped.
pub struct SerializeContext {
    slots: BTreeMap<ComponentKey, Slot>,
    filter: Box<dyn StaticFilter>,
}

impl SerializeContext {
    /// Construct a a new serializer context
    pub fn builder() -> SerializeBuilder<Without> {
        SerializeBuilder::new()
    }

    /// Serialize the world in a column major format.
    /// This is more efficient but less human readable.
    pub fn serialize<'a>(
        &'a self,
        world: &'a World,
        format: SerializeFormat,
    ) -> WorldSerializer<'a> {
        WorldSerializer {
            format,
            world,
            context: self,
        }
    }

    fn archetypes<'a>(
        &'a self,
        world: &'a World,
    ) -> impl Iterator<Item = (ArchetypeId, &'a Archetype)> {
        world.archetypes().filter(|(_, arch)| {
            !arch.is_empty()
                && arch.storage().keys().any(|id| self.slots.contains_key(id))
                && self.filter.static_matches(arch)
        })
    }
}

/// Serializes the world
pub struct WorldSerializer<'a> {
    format: SerializeFormat,
    context: &'a SerializeContext,
    world: &'a World,
}

impl<'a> Serialize for WorldSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.format {
            SerializeFormat::RowMajor => {
                let mut state = serializer.serialize_struct_variant("World", 0, "row", 1)?;
                state.serialize_field(
                    "entities",
                    &SerializeEntities {
                        world: self.world,
                        context: self.context,
                    },
                )?;
                state.end()
            }
            SerializeFormat::ColumnMajor => {
                let mut state = serializer.serialize_struct_variant("World", 1, "col", 1)?;
                state.serialize_field(
                    "archetypes",
                    &SerializeArchetypes {
                        world: self.world,
                        context: self.context,
                    },
                )?;
                state.end()
            }
        }
    }
}

struct SerializeEntities<'a> {
    world: &'a World,
    context: &'a SerializeContext,
}

impl<'a> Serialize for SerializeEntities<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = self
            .context
            .archetypes(self.world)
            .map(|(_, v)| v.len())
            .sum();

        let mut seq = serializer.serialize_seq(Some(len))?;

        for (_, arch) in self.context.archetypes(self.world) {
            for slot in arch.slots() {
                seq.serialize_element(&SerializeEntity {
                    slot,
                    arch,
                    id: arch.entity(slot).expect("Invalid slot"),
                    context: self.context,
                })?;
            }
        }

        seq.end()
    }
}

struct SerializeEntity<'a> {
    slot: usize,
    arch: &'a Archetype,
    id: Entity,
    context: &'a SerializeContext,
}

impl<'a> Serialize for SerializeEntity<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_tuple_struct("Entity", 2)?;
        state.serialize_field(&self.id)?;
        state.serialize_field(&SerializeEntityData {
            slot: self.slot,
            arch: self.arch,
            context: self.context,
        })?;

        state.end()
    }
}

struct SerializeEntityData<'a> {
    slot: usize,
    arch: &'a Archetype,
    context: &'a SerializeContext,
}

impl<'a> Serialize for SerializeEntityData<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = self
            .arch
            .storage()
            .keys()
            .filter(|key| self.context.slots.contains_key(key))
            .count();

        let mut state = serializer.serialize_map(Some(len))?;
        for storage in self.arch.borrow_all() {
            if let Some(slot) = self.context.slots.get(&storage.info().id()) {
                state.serialize_entry(&slot.key, (slot.ser)(&storage, self.slot))?;
            }
        }

        state.end()
    }
}

struct SerializeArchetypes<'a> {
    world: &'a World,
    context: &'a SerializeContext,
}

impl<'a> serde::Serialize for SerializeArchetypes<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state =
            serializer.serialize_seq(Some(self.context.archetypes(self.world).count()))?;

        for (_, arch) in self.context.archetypes(self.world) {
            state.serialize_element(&SerializeArchetype {
                context: self.context,
                arch,
            })?;
        }

        state.end()
    }
}

struct SerializeArchetype<'a> {
    arch: &'a Archetype,
    context: &'a SerializeContext,
}

struct SerializeStorages<'a> {
    arch: &'a Archetype,
    context: &'a SerializeContext,
}

struct SerializeStorage<'a> {
    storage: &'a StorageBorrowDyn<'a>,
    slot: &'a Slot,
}

impl<'a> serde::Serialize for SerializeStorage<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let ser_fn = self.slot.ser;
        let mut seq = serializer.serialize_seq(Some(self.storage.len()))?;
        for slot in 0..self.storage.len() {
            seq.serialize_element(ser_fn(self.storage, slot))?;
        }

        seq.end()
    }
}
impl<'a> serde::Serialize for SerializeStorages<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let len = self
            .arch
            .storage()
            .keys()
            .filter(|key| self.context.slots.contains_key(key))
            .count();

        let mut state = serializer.serialize_map(Some(len))?;

        for storage in self.arch.borrow_all() {
            let id = storage.info().id;
            if let Some(slot) = self.context.slots.get(&id) {
                state.serialize_entry(
                    &slot.key,
                    &SerializeStorage {
                        storage: &storage,
                        slot,
                    },
                )?;
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
        state.serialize_field(&SerializeStorages {
            arch: self.arch,
            context: self.context,
        })?;

        state.end()
    }
}
