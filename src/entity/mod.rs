mod store;

use core::fmt;
use core::num::NonZeroU64;
use std::fmt::Display;
use std::num::NonZeroU32;
use std::sync::atomic::AtomicU32;

use crate::util::Key;

pub use store::*;

#[derive(Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
#[repr(transparent)]
/// Represents an entity.
/// An entity can either declare an identifier spawned into the world,
/// a static entity or component, or a typed relation between two entities.
///
/// # Structure

/// | 16         | 4    | 28    |
/// | Generation | Kind | Index |
///
/// The Index is always NonZero.
///
/// An entity id retains uniqueness if cast to a u32. To allow global static
/// entities to coexist as the flags are kept.
pub struct Entity(NonZeroU64);

const INDEX_MASK: u64 = /*     */ 0x000000000FFFFFFF;
const GENERATION_MASK: u64 = /**/ 0x0000FFFF00000000;
const KIND_MASK: u64 = /*     */ 0x00000000F0000000;

bitflags::bitflags! {
    /// Flags for an entity.
    /// Can not exceed 4 bits
    pub struct EntityKind: u8 {
        const STATIC = 1;
        const COMPONENT = 2;
    }
}

impl Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_empty() {
            write!(f, "{self:?}")
        } else {
            Ok(())
        }
    }
}

static STATIC_IDS: AtomicU32 = AtomicU32::new(1);

pub type Generation = u16;
pub type EntityIndex = NonZeroU32;

impl Entity {
    /// Generate a new static id
    pub fn acquire_static_id(kind: EntityKind) -> Entity {
        let index = STATIC_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Entity::from_parts(NonZeroU32::new(index).unwrap(), 0, kind)
    }

    pub fn index(self) -> EntityIndex {
        // Can only be constructed from parts
        NonZeroU32::new((self.0.get() & INDEX_MASK) as u32).unwrap()
    }

    pub fn generation(self) -> Generation {
        ((self.0.get() & GENERATION_MASK) >> 32) as Generation
    }

    pub fn flags(self) -> EntityKind {
        EntityKind::from_bits_truncate(((self.0.get() & KIND_MASK) >> 28) as u8)
    }

    pub fn into_parts(self) -> (EntityIndex, Generation, EntityKind) {
        let bits = self.0.get();

        (
            NonZeroU32::new((bits & INDEX_MASK) as u32).unwrap(),
            ((bits & GENERATION_MASK) >> 32) as Generation,
            EntityKind::from_bits_truncate(((bits & KIND_MASK) >> 28) as u8),
        )
    }

    pub fn from_parts(index: EntityIndex, gen: Generation, kind: EntityKind) -> Self {
        assert!(index.get() < (u32::MAX >> 1));
        let bits = ((index.get() as u64) & INDEX_MASK)
            | ((gen as u64) << 32)
            | ((kind.bits() as u64) << 28);

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
        let (index, generation, flags) = self.into_parts();
        if self.flags().is_empty() {
            write!(f, "{index}:{generation}")
        } else {
            write!(f, "{flags}:{index}:{generation}")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{
        entity::{EntityKind, EntityLocation},
        Entity,
    };

    use super::EntityStore;
    #[test]
    fn entity_store() {
        let mut entities = EntityStore::new();
        let a = entities.spawn(EntityLocation {
            archetype: 0,
            slot: 4,
        });
        let b = entities.spawn(EntityLocation {
            archetype: 3,
            slot: 2,
        });
        let c = entities.spawn(EntityLocation {
            archetype: 5,
            slot: 3,
        });

        entities.despawn(b);

        eprintln!("Despawning: {b:?}");
        assert!(entities.is_alive(a));
        assert!(!entities.is_alive(b));
        assert!(entities.is_alive(c));
        assert_eq!(
            entities.get(c),
            Some(&EntityLocation {
                archetype: 5,
                slot: 3
            })
        );
        assert_eq!(entities.get(b), None);
    }

    #[test]
    fn entity_id() {
        let parts = (NonZeroU32::new(0xFFF).unwrap(), 30, EntityKind::COMPONENT);

        let a = Entity::from_parts(parts.0, parts.1, EntityKind::COMPONENT);

        eprintln!("a: {:b}", a.0.get());

        assert_eq!(parts.0, a.index());
        assert_eq!(parts, a.into_parts());
    }
}
