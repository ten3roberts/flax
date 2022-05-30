mod builder;
mod store;

use core::fmt;
use core::num::NonZeroU64;
use std::num::NonZeroU32;
use std::sync::atomic::AtomicU32;

pub use builder::*;
pub use store::*;

use crate::EntityFetch;

#[derive(Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
#[repr(transparent)]
/// Represents an entity.
/// An entity can either declare an identifier spawned into the world,
/// a static entity or component, or a typed relation between two entities.
///
/// # Structure

/// | 16         | 24    | 8         |
/// | Generation | Index | Namespace |
///
/// The Index is always NonZero.
///
/// An entity id retains uniqueness if cast to a u32. To allow global static
/// entities to coexist as the flags are kept.
pub struct Entity(NonZeroU64);

const INDEX_MASK: u64 = /*     */ 0x00000000FFFFFF00;
const GENERATION_MASK: u64 = /**/ 0x0000FFFF00000000;
// A simple u8 cast will suffice
const _NAMESPACE_MASK: u64 = /* */ 0x00000000000000FF;

static STATIC_IDS: AtomicU32 = AtomicU32::new(1);

pub type Generation = u16;
pub type EntityIndex = NonZeroU32;
pub type Namespace = u8;

pub const STATIC_NAMESPACE: Namespace = 255;

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
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (index, generation, kind) = self.into_parts();
        f.debug_tuple("Entity")
            .field(&index)
            .field(&generation)
            .field(&kind)
            .finish()
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (index, generation, namespace) = self.into_parts();
        write!(f, "{namespace}:{index}:{generation}")
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
