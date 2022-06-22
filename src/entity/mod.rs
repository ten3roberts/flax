mod builder;
mod store;

use core::fmt;
use core::num::NonZeroU64;
use std::num::NonZeroU32;
use std::sync::atomic::AtomicU32;

pub use builder::*;
pub use store::*;

use crate::{component, EntityFetch};

/// Represents an entity.
/// An entity can either declare an identifier spawned into the world,
/// a static entity or component, or a typed relation between two entities.
///
/// # Structure

/// | 16       | 16         | 24    | 8         |
/// | Reserved | Generation | Index | Namespace |
///
/// The Index is always NonZero.
///
/// An entity id retains uniqueness if cast to a u32. To allow global static
/// entities to coexist as the flags are kept.
///
/// Pairs
/// An entity can be associated to another entity. In such a case, the upper
/// bits store the associated entity.
/// This means that the generation safeguard is removed. This is not problematic
/// for storage as pair will be removed when the base is removed. Furthermore,
/// pairs are often declared statically which means they can't be freed
#[derive(PartialOrd, Clone, Copy, PartialEq, Eq, Ord, Hash)]
#[repr(transparent)]
pub struct Entity(NonZeroU64);
/// Same as [crate::Entity] but without generation.
#[derive(Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct StrippedEntity(NonZeroU32);

const INDEX_MASK: u64 = /*      */ 0x00000000FFFFFF00;
const GENERATION_MASK: u64 = /* */ 0x0000FFFF00000000;
// A simple u8 cast will suffice
const _NAMESPACE_MASK: u64 = /* */ 0x00000000000000FF;

static STATIC_IDS: AtomicU32 = AtomicU32::new(1);

pub type Generation = u16;
pub type EntityIndex = NonZeroU32;
pub type Namespace = u8;

pub const STATIC_NAMESPACE: Namespace = 255;

component! {
    /// The object for a pair component which will match anything.
    pub wildcard: (),
}

impl Entity {
    /// Generate a new static id
    pub fn acquire_static_id() -> Entity {
        let index = STATIC_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Entity::from_parts(NonZeroU32::new(index).unwrap(), 0, STATIC_NAMESPACE)
    }

    pub fn index(self) -> EntityIndex {
        // Can only be constructed from parts
        NonZeroU32::new(((self.0.get() & INDEX_MASK) >> 8) as u32).unwrap()
    }

    pub fn generation(self) -> Generation {
        ((self.0.get() & GENERATION_MASK) >> 32) as Generation
    }

    pub fn namespace(self) -> Namespace {
        self.0.get() as u8
    }

    pub fn into_parts(self) -> (EntityIndex, Generation, Namespace) {
        let bits = self.0.get();

        (
            NonZeroU32::new(((bits & INDEX_MASK) >> 8) as u32).unwrap(),
            ((bits & GENERATION_MASK) >> 32) as Generation,
            bits as u8,
        )
    }

    pub fn from_parts(index: EntityIndex, gen: Generation, namespace: Namespace) -> Self {
        assert!(index.get() < (u32::MAX >> 1));
        let bits =
            (((index.get() as u64) << 8) & INDEX_MASK) | ((gen as u64) << 32) | (namespace as u64);

        Self(NonZeroU64::new(bits).unwrap())
    }

    pub fn from_bits(bits: NonZeroU64) -> Self {
        Self(bits)
    }

    pub fn to_bits(&self) -> NonZeroU64 {
        self.0
    }

    pub fn pair(subject: Entity, object: Entity) -> Self {
        let a = subject.to_bits().get();
        let b = object.to_bits().get();

        Self(NonZeroU64::new((a & 0xFFFFFFFF) | (b << 32)).unwrap())
    }

    pub fn into_pair(self) -> (StrippedEntity, StrippedEntity) {
        let bits = self.to_bits().get();
        let subject = StrippedEntity(NonZeroU32::new(bits as u32).unwrap());
        let object = StrippedEntity(NonZeroU32::new((bits >> 32) as u32).unwrap());

        (subject, object)
    }

    pub fn strip_gen(self) -> StrippedEntity {
        StrippedEntity(NonZeroU32::new(self.to_bits().get() as u32).unwrap())
    }

    pub fn builder() -> EntityBuilder {
        EntityBuilder::new()
    }
}

impl StrippedEntity {
    pub fn index(self) -> EntityIndex {
        // Can only be constructed from parts
        NonZeroU32::new(((self.0.get() & INDEX_MASK as u32) >> 8) as u32).unwrap()
    }

    pub fn namespace(self) -> Namespace {
        self.0.get() as u8
    }

    /// Reconstruct a generationless entity with a generation
    pub fn reconstruct(self, gen: Generation) -> Entity {
        Entity(NonZeroU64::new((self.0.get() as u64) | ((gen as u64) << 32)).unwrap())
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (index, generation, namespace) = self.into_parts();
        f.debug_tuple("Entity")
            .field(&index)
            .field(&generation)
            .field(&namespace)
            .finish()
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (index, generation, namespace) = self.into_parts();
        write!(f, "{namespace}:{index}:{generation}")
    }
}

impl fmt::Debug for StrippedEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let index = self.index();
        let namespace = self.namespace();

        f.debug_tuple("StrippedEntity")
            .field(&namespace)
            .field(&index)
            .field(&"_")
            .finish()
    }
}

impl fmt::Display for StrippedEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let index = self.index();
        let namespace = self.namespace();
        write!(f, "{namespace}:{index}:_")
    }
}

/// Access the entity ids in a query
pub fn entities() -> EntityFetch {
    EntityFetch
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{archetype::Archetype, entity::EntityLocation, Entity};

    use super::EntityStore;
    #[test]
    fn entity_store() {
        let mut entities = EntityStore::new(1);
        let arch = EntityStore::new(2).spawn(Archetype::empty());

        let a = entities.spawn(EntityLocation { arch, slot: 4 });
        let b = entities.spawn(EntityLocation { arch, slot: 2 });
        let c = entities.spawn(EntityLocation { arch, slot: 3 });

        entities.despawn(b);

        eprintln!("Despawning: {b:?}");
        assert!(entities.is_alive(a));
        assert!(!entities.is_alive(b));
        assert!(entities.is_alive(c));
        assert_eq!(entities.get(c), Some(&EntityLocation { arch, slot: 3 }));
        assert_eq!(entities.get(b), None);
    }

    #[test]
    fn entity_id() {
        let parts = (NonZeroU32::new(23298).unwrap(), 30, 1);

        let a = Entity::from_parts(parts.0, parts.1, parts.2);

        eprintln!("a: {:b}", a.0.get());

        assert_eq!(parts.0, a.index());
        assert_eq!(parts, a.into_parts());
    }
}
