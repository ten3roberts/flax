use core::marker::PhantomData;
use itertools::Itertools;
use once_cell::sync::Lazy;

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};

use crate::{
    archetype::ArchetypeStorage,
    component::{ComponentDesc, ComponentValue},
    components::name,
    serialize::StorageVisitor,
    Component, EntityBuilder,
};

use super::{DeserializeColFn, DeserializeRowFn, SerializeFn};

pub(super) fn deser_col<T: ComponentValue + for<'x> Deserialize<'x>>(
    deserializer: &mut dyn erased_serde::Deserializer,
    desc: ComponentDesc,
    len: usize,
) -> erased_serde::Result<ArchetypeStorage> {
    deserializer.deserialize_seq(StorageVisitor::<T> {
        desc,
        cap: len,
        _marker: PhantomData,
    })
}

pub(super) fn deser_one<T: ComponentValue + for<'x> Deserialize<'x>>(
    deserializer: &mut dyn erased_serde::Deserializer,
    desc: ComponentDesc,
    builder: &mut EntityBuilder,
) -> erased_serde::Result<()> {
    let value = T::deserialize(deserializer)?;
    builder.set(desc.downcast(), value);
    Ok(())
}

pub(super) fn ser<T: ComponentValue + Serialize>(
    storage: &ArchetypeStorage,
    slot: usize,
) -> &dyn erased_serde::Serialize {
    &storage.downcast_ref::<T>()[slot]
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct ComponentSerializerPlugin {
    pub(crate) desc: fn() -> ComponentDesc,
    pub(crate) serialize_fn: SerializeFn,
}

impl ComponentSerializerPlugin {
    pub const fn new<T: ComponentValue + Serialize>(
        _: fn() -> Component<T>,
        desc: fn() -> ComponentDesc,
    ) -> Self {
        Self {
            desc,
            serialize_fn: ser::<T>,
        }
    }
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct ComponentDeserializerPlugin {
    pub(crate) desc: fn() -> ComponentDesc,
    pub(crate) deserialize_col_fn: DeserializeColFn,
    pub(crate) deserialize_row_fn: DeserializeRowFn,
}

impl ComponentDeserializerPlugin {
    pub const fn new<T: ComponentValue + DeserializeOwned>(
        _: fn() -> Component<T>,
        desc: fn() -> ComponentDesc,
    ) -> Self {
        Self {
            desc,
            deserialize_row_fn: |de, desc, builder| {
                let value = erased_serde::deserialize::<T>(de)?;
                builder.set(desc.downcast::<T>(), value);

                Ok(())
            },
            deserialize_col_fn: deser_col::<T>,
        }
    }
}

pub(crate) struct ComponentRegistry {
    serializers: Vec<ComponentSerializerPlugin>,
    deserializers: Vec<ComponentDeserializerPlugin>,
}

impl ComponentRegistry {
    pub(crate) fn serializers(&self) -> &[ComponentSerializerPlugin] {
        &self.serializers
    }

    pub(crate) fn deserializers(&self) -> &[ComponentDeserializerPlugin] {
        &self.deserializers
    }
}

pub(super) static REGISTRY: Lazy<ComponentRegistry> = Lazy::new(|| {
    let serializers = inventory::iter::<ComponentSerializerPlugin>()
        .copied()
        .collect_vec();

    let deserializers = inventory::iter::<ComponentDeserializerPlugin>()
        .copied()
        .collect_vec();

    ComponentRegistry {
        serializers,
        deserializers,
    }
});

inventory::collect!(ComponentSerializerPlugin);
inventory::collect!(ComponentDeserializerPlugin);

#[macro_export]
/// Register a serializable and deserializable component to the global registry
macro_rules! register_serializable {
    ($($component: ident),*) => {
        $(
            $crate::__internal::inventory::submit! {
                $crate::serialize::registry::ComponentSerializerPlugin::new($component, || $component().desc())
            }
            $crate::__internal::inventory::submit! {
                $crate::serialize::registry::ComponentDeserializerPlugin::new($component, || $component().desc())
            }
        )*
    };
}

#[macro_export]
/// Register a serializable component to the global registry
macro_rules! register_serializable_only {
    ($($component: ident),*) => {
        $(
            $crate::__internal::inventory::submit! {
                $crate::serialize::registry::ComponentSerializerPlugin::new($component, || $component().desc())
            }
        )*
    };
}

#[macro_export]
/// Register a deserializable component to the global registry
macro_rules! register_deserializable_only {
    ($($component: ident),*) => {
        $(
            $crate::__internal::inventory::submit! {
                $crate::serialize::registry::ComponentDeserializerPlugin::new($component, || $component().desc())
            }
        )*
    };
}

register_serializable!(name);
