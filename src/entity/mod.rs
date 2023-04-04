mod builder;
mod store;

use core::fmt;
use core::num::NonZeroU16;
use core::sync::atomic::{AtomicU32, Ordering};

pub use builder::*;
pub(crate) use store::*;

use crate::EntityIds;

pub(crate) const DEFAULT_GEN: EntityGen = unsafe { EntityGen::new_unchecked(1) };

/// Represents an entity identifier.
/// An entity can either declare an identifier spawned into the world,
/// a static entity, or a component.
#[derive(PartialOrd, Clone, Copy, PartialEq, Eq, Ord, Hash)]
pub struct Entity {
    pub(crate) index: EntityIndex,
    /// Object
    pub(crate) gen: EntityGen,
    pub(crate) kind: EntityKind,
}

impl Entity {
    /// The lowest possible entity
    ///
    /// May or may not refer to a valid entity.
    pub(crate) const MIN: Self = unsafe {
        Entity {
            index: 0,
            gen: NonZeroU16::new_unchecked(1),
            kind: EntityKind::empty(),
        }
    };

    /// The greatest possible entity
    ///
    /// May or may not refer to a valid entity.
    pub(crate) const MAX: Self = unsafe {
        Entity {
            index: u32::MAX,
            gen: NonZeroU16::new_unchecked(u16::MAX),
            kind: EntityKind::all(),
        }
    };

    pub(crate) fn from_parts(index: EntityIndex, gen: EntityGen, kind: EntityKind) -> Self {
        Self { index, gen, kind }
    }

    /// Creates a new entity builder.
    /// See [crate::EntityBuilder] for more details.
    pub fn builder() -> EntityBuilder {
        EntityBuilder::new()
    }

    /// Returns true if the id is a static id
    pub fn is_static(&self) -> bool {
        self.kind.contains(EntityKind::STATIC)
    }

    /// Returns true if the id is a component id
    pub fn is_component(&self) -> bool {
        self.kind.contains(EntityKind::COMPONENT)
    }
    ///
    /// Generate a new static id
    pub fn acquire_static_id(kind: EntityKind) -> Entity {
        let index = STATIC_IDS.fetch_add(1, Ordering::Relaxed);
        Entity::from_parts(index, DEFAULT_GEN, kind | EntityKind::STATIC)
    }

    #[doc(hidden)]
    pub fn static_init(id: &AtomicU32, kind: EntityKind) -> Self {
        let index = match id.fetch_update(Ordering::Acquire, Ordering::Relaxed, |v| {
            if v != EntityIndex::MAX {
                None
            } else {
                Some(Self::acquire_static_id(kind | EntityKind::STATIC).index())
            }
        }) {
            Ok(_) => id.load(Ordering::Acquire),
            Err(old) => old,
        };

        Self::from_parts(index, DEFAULT_GEN, kind | EntityKind::STATIC)
    }

    /// Returns the entity index
    #[inline(always)]
    pub fn index(&self) -> EntityIndex {
        self.index
    }

    /// Returns the entity generation
    #[inline(always)]
    pub fn gen(&self) -> EntityGen {
        self.gen
    }

    /// Returns the entity kind
    #[inline(always)]
    pub fn kind(&self) -> EntityKind {
        self.kind
    }
}

#[cfg(feature = "serde")]
mod serde_impl {
    use serde::{
        de::{self, Unexpected, Visitor},
        ser::SerializeTupleStruct,
        Deserialize, Serialize,
    };

    use super::{Entity, EntityKind};

    impl Serialize for EntityKind {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            self.bits().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for EntityKind {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_u16(EntityKindVisitor)
        }
    }

    struct EntityKindVisitor;

    impl<'de> Visitor<'de> for EntityKindVisitor {
        type Value = EntityKind;

        fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            write!(f, "A valid entity kind bitfield")
        }

        fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            EntityKind::from_bits(v)
                .ok_or_else(|| de::Error::invalid_value(Unexpected::Unsigned(v as _), &self))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            EntityKind::from_bits(v as _)
                .ok_or_else(|| de::Error::invalid_value(Unexpected::Unsigned(v as _), &self))
        }
    }

    impl Serialize for Entity {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut state = serializer.serialize_tuple_struct("Entity", 3)?;
            state.serialize_field(&self.index)?;
            state.serialize_field(&self.gen)?;
            state.serialize_field(&self.kind)?;
            state.end()
        }
    }

    struct EntityVisitor;

    impl<'de> Deserialize<'de> for Entity {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_tuple_struct("Entity", 3, EntityVisitor)
        }
    }

    impl<'de> Visitor<'de> for EntityVisitor {
        type Value = Entity;

        fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
            write!(formatter, "a sequence of entity parts")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let index = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(0, &self))?;
            let gen = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(1, &self))?;
            let kind = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(2, &self))?;

            Ok(Entity::from_parts(index, gen, kind))
        }
    }
}

static STATIC_IDS: AtomicU32 = AtomicU32::new(1);

bitflags::bitflags! {
    /// Declares the roles an entity id serves
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct EntityKind: u16 {
        /// The entity is a component
        const COMPONENT = 1;
        /// The entity is created via static initialization and is never
        /// despawned
        const STATIC = 2;
    }
}

impl Default for EntityKind {
    fn default() -> Self {
        Self::empty()
    }
}

/// The entity id version
pub type EntityGen = NonZeroU16;
/// The index of the entity in the entity store
pub type EntityIndex = u32;

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { index, gen, kind } = *self;
        if kind.is_empty() {
            write!(f, "{index}v{gen}")
        } else {
            write!(f, "{index}v{gen} [{kind:?}]")
        }
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Access the entity ids in a query
#[inline]
pub fn entity_ids() -> EntityIds {
    EntityIds
}

#[cfg(test)]
mod tests {

    use core::mem::{align_of, size_of};

    use crate::{entity::EntityKind, Entity};

    use super::EntityStore;
    #[test]
    fn entity_store() {
        let mut store = EntityStore::new(EntityKind::COMPONENT);

        let a = store.spawn("a");
        let b = store.spawn("b");
        let c = store.spawn("c");

        store.despawn(b).unwrap();

        // eprintln!("Despawning: {b:?}");
        assert!(store.is_alive(a));
        assert!(!store.is_alive(b));
        assert!(store.is_alive(c));
        assert_eq!(store.get(c), Some(&"c"));
        assert_eq!(store.get(b), None);

        let d = store.spawn("d");
        assert_eq!(d.index(), b.index());

        assert!(store.get(b).is_none());
        assert_eq!(store.get(d), Some(&"d"));
    }

    #[test]
    fn entity_size() {
        assert_eq!(size_of::<Entity>(), 8);
        assert_eq!(align_of::<Entity>(), 4);
        assert_eq!(size_of::<Option<Entity>>(), 8);
    }
}
