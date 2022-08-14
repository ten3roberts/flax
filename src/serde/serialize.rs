use std::collections::BTreeMap;

use crate::{
    archetype::StorageBorrowDyn, components::is_component, And, Archetype, Archetypes, Component,
    ComponentId, ComponentValue, StaticFilter, Without, World,
};

use serde::{
    ser::{SerializeMap, SerializeSeq, SerializeStruct, SerializeTupleStruct},
    Serializer,
};

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    ser_col: for<'x> fn(
        storage: &'x StorageBorrowDyn<'_>,
        slot: usize,
    ) -> &'x dyn erased_serde::Serialize,
    key: String,
}

#[derive(Clone)]
pub struct SerializeBuilder<F> {
    slots: BTreeMap<ComponentId, Slot>,
    filter: F,
}

impl SerializeBuilder<Without> {
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
    pub fn with<T: ComponentValue + serde::Serialize>(
        &mut self,
        key: impl Into<String>,
        component: Component<T>,
    ) -> &mut Self {
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
            component.id(),
            Slot {
                key: key.into(),
                ser_col: ser_col::<T>,
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
    slots: BTreeMap<ComponentId, Slot>,
    filter: Box<dyn StaticFilter>,
}

impl SerializeContext {
    /// Construct a a new serializer context
    pub fn builder() -> SerializeBuilder<Without> {
        SerializeBuilder::new()
    }

    /// Serialize the world in a column major format.
    /// This is more efficient but less human readable.
    pub fn serialize<'a>(&'a self, world: &'a World) -> WorldSerializer<'a> {
        WorldSerializer {
            world,
            context: self,
        }
    }
}

pub struct WorldSerializer<'a> {
    world: &'a World,
    context: &'a SerializeContext,
}

impl<'a> serde::Serialize for WorldSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("World", 3)?;
        state.serialize_field(
            "archetypes",
            &SerializeArchetypes {
                archetypes: &self.world.archetypes,
                context: self.context,
            },
        )?;

        state.end()
    }
}

struct SerializeArchetypes<'a> {
    archetypes: &'a Archetypes,
    context: &'a SerializeContext,
}

impl<'a> serde::Serialize for SerializeArchetypes<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_seq(Some(self.archetypes.len()))?;

        for (_, arch) in self.archetypes.iter() {
            if !arch.is_empty()
                && arch
                    .component_ids()
                    .any(|id| self.context.slots.contains_key(&id))
                && self.context.filter.static_matches(arch)
            {
                state.serialize_element(&SerializeArchetype {
                    context: self.context,
                    arch,
                })?;
            }
        }

        state.end()
    }
}

struct SerializeArchetype<'a> {
    arch: &'a Archetype,
    context: &'a SerializeContext,
}

struct SerializeStorages<'a> {
    storage: &'a Archetype,
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
        let ser_fn = self.slot.ser_col;
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
        let mut state = serializer.serialize_map(None)?;

        for storage in self.storage.storages() {
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
            storage: self.arch,
            context: self.context,
        })?;

        state.end()
    }
}
