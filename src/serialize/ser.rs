use crate::{
    archetype::{Archetype, ArchetypeId, ArchetypeStorage},
    component::{ComponentKey, ComponentValue},
    components::component_info,
    filter::StaticFilter,
    Component, Entity, EntityRef, World,
};

use alloc::{collections::BTreeMap, string::String};
use serde::{
    ser::{SerializeMap, SerializeSeq, SerializeStructVariant, SerializeTupleStruct},
    Serialize, Serializer,
};

use super::{registry::REGISTRY, SerializeFormat};

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    ser: for<'x> fn(storage: &'x ArchetypeStorage, slot: usize) -> &'x dyn erased_serde::Serialize,
    key: String,
}

#[derive(Clone)]
/// Builder for a serialialization context
pub struct SerializeBuilder {
    slots: BTreeMap<ComponentKey, Slot>,
}

impl SerializeBuilder {
    /// Creates a new SerializeBuilder
    pub fn new() -> Self {
        Self {
            slots: Default::default(),
        }
    }
}

impl Default for SerializeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SerializeBuilder {
    /// Creates a component deserializer from the global registry
    pub fn from_registry(&mut self) -> &mut Self {
        self.slots.extend(REGISTRY.serializers().iter().map(|v| {
            let desc = (v.desc)();
            (
                desc.key(),
                Slot {
                    ser: v.serialize_fn,
                    key: desc.name().to_string(),
                },
            )
        }));

        self
    }

    /// Register a component using the component name.
    ///
    /// See u`Self::with_name`u
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
        fn ser_col<T: serde::Serialize + ComponentValue + Sized>(
            storage: &ArchetypeStorage,
            slot: usize,
        ) -> &dyn erased_serde::Serialize {
            &storage.downcast_ref::<T>()[slot]
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

    /// Finish constructing the serialization context
    pub fn build(&mut self) -> SerializeContext {
        SerializeContext {
            slots: self.slots.clone(),
        }
    }
}

/// Describes how to serialize a world given a group of components to serialize
/// and an optional filter. Empty entities will be skipped.
pub struct SerializeContext {
    slots: BTreeMap<ComponentKey, Slot>,
}

impl SerializeContext {
    /// Construct a new serializer context
    pub fn builder() -> SerializeBuilder {
        SerializeBuilder::new()
    }

    /// Serialize the world in a column major format.
    /// This is more efficient but less human readable.
    pub fn serialize_world<'a, F: StaticFilter>(
        &'a self,
        world: &'a World,
        format: SerializeFormat,
        filter: F,
    ) -> WorldSerializer<'a, F> {
        WorldSerializer {
            format,
            world,
            context: self,
            filter,
        }
    }

    /// Serialize a single entity
    pub fn serialize_entity<'a>(&'a self, entity: &EntityRef<'a>) -> EntityDataSerializer<'a> {
        EntityDataSerializer {
            slot: entity.loc.slot,
            arch: entity.arch,
            context: self,
        }
    }

    fn archetypes<'a>(
        &'a self,
        world: &'a World,
        filter: &'a impl StaticFilter,
    ) -> impl Iterator<Item = (ArchetypeId, &'a Archetype)> + 'a {
        world.archetypes.iter().filter(move |(_, arch)| {
            !arch.is_empty()
                && arch
                    .components()
                    .keys()
                    .any(|id| self.slots.contains_key(id))
                && !arch.has(component_info().key())
                && filter.filter_static(arch)
        })
    }
}

/// Serializes the world
pub struct WorldSerializer<'a, F> {
    format: SerializeFormat,
    context: &'a SerializeContext,
    world: &'a World,
    filter: F,
}

impl<F: StaticFilter> Serialize for WorldSerializer<'_, F> {
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
                        filter: &self.filter,
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
                        filter: &self.filter,
                    },
                )?;
                state.end()
            }
        }
    }
}

struct SerializeEntities<'a, F> {
    world: &'a World,
    context: &'a SerializeContext,
    filter: &'a F,
}

impl<F: StaticFilter> Serialize for SerializeEntities<'_, F> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = self
            .context
            .archetypes(self.world, self.filter)
            .map(|(_, v)| v.len())
            .sum();

        let mut seq = serializer.serialize_seq(Some(len))?;

        for (_, arch) in self.context.archetypes(self.world, self.filter) {
            for slot in arch.slots() {
                seq.serialize_element(&EntityIdSerializer {
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

/// Serializes an entity
struct EntityIdSerializer<'a> {
    slot: usize,
    arch: &'a Archetype,
    id: Entity,
    context: &'a SerializeContext,
}

impl Serialize for EntityIdSerializer<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_tuple_struct("Entity", 2)?;
        state.serialize_field(&self.id)?;
        state.serialize_field(&EntityDataSerializer {
            slot: self.slot,
            arch: self.arch,
            context: self.context,
        })?;

        state.end()
    }
}

/// Serializes the components of an entity
pub struct EntityDataSerializer<'a> {
    slot: usize,
    arch: &'a Archetype,
    context: &'a SerializeContext,
}

impl Serialize for EntityDataSerializer<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = self
            .arch
            .components()
            .keys()
            .filter(|key| self.context.slots.contains_key(key))
            .count();

        let mut state = serializer.serialize_map(Some(len))?;
        for cell in self.arch.cells() {
            let data = cell.data.borrow();
            if let Some(slot) = self.context.slots.get(&data.key) {
                state.serialize_entry(&slot.key, (slot.ser)(&data.storage, self.slot))?;
            }
        }

        state.end()
    }
}

struct SerializeArchetypes<'a, F> {
    world: &'a World,
    context: &'a SerializeContext,
    filter: &'a F,
}

impl<F: StaticFilter> serde::Serialize for SerializeArchetypes<'_, F> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_seq(Some(
            self.context.archetypes(self.world, self.filter).count(),
        ))?;

        for (_, arch) in self.context.archetypes(self.world, self.filter) {
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
    storage: &'a ArchetypeStorage,
    slot: &'a Slot,
}

impl serde::Serialize for SerializeStorage<'_> {
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

impl serde::Serialize for SerializeStorages<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let len = self
            .arch
            .components()
            .keys()
            .filter(|key| self.context.slots.contains_key(key))
            .count();

        let mut state = serializer.serialize_map(Some(len))?;

        for cell in self.arch.cells() {
            let data = cell.data.borrow();

            let id = data.key;
            if let Some(slot) = self.context.slots.get(&id) {
                state.serialize_entry(
                    &slot.key,
                    &SerializeStorage {
                        storage: &data.storage,
                        slot,
                    },
                )?;
            }
        }

        state.end()
    }
}

impl serde::Serialize for SerializeArchetype<'_> {
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
