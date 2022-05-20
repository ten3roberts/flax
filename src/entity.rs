use core::fmt;
use core::num::NonZeroU64;
use std::fmt::Display;
use std::mem::ManuallyDrop;
use std::num::NonZeroU32;
use std::sync::atomic::AtomicU32;

use crate::archetype::ArchetypeId;
use crate::util::Key;

#[derive(Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
#[repr(transparent)]
/// Represents an entity.
/// An entity can either declare an identifier spawned into the world,
/// a static entity or component, or a typed relation between two entities.
///
/// # Structure
/// | 31    | 1       | 16         | 16   |
/// | Index | Dynamic | Generation | Kind |
///
/// The Index is always NonZero.
///
/// The Index part is always non-zero.
///
/// Any entity with a generatio = 0 is considered static
pub struct Entity(NonZeroU64);

// A simple u32 cast is used instead
const _INDEX_MASK: u64 = /*     */ 0x00000000FFFFFFFF;
const GENERATION_MASK: u64 = /**/ 0x0000FFFF00000000;
const KIND_MASK: u64 = /*      */ 0xFFFF000000000000;

bitflags::bitflags! {
    pub struct EntityFlags: u16  {

    }
}

impl Display for EntityFlags {
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
    pub fn acquire_static_id(kind: EntityFlags) -> Entity {
        let index = STATIC_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Entity::from_parts(NonZeroU32::new(index).unwrap(), 0, kind)
    }

    pub fn index(self) -> EntityIndex {
        // Can only be constructed from parts
        NonZeroU32::new(self.0.get() as u32).unwrap()
    }

    pub fn generation(self) -> Generation {
        ((self.0.get() & GENERATION_MASK) >> 32) as Generation
    }

    pub fn flags(self) -> EntityFlags {
        EntityFlags::from_bits_truncate(((self.0.get() & KIND_MASK) >> 48) as u16)
    }

    pub fn into_parts(self) -> (EntityIndex, Generation, EntityFlags) {
        let bits = self.0.get();

        (
            NonZeroU32::new(bits as u32).unwrap(),
            ((bits & GENERATION_MASK) >> 32) as Generation,
            EntityFlags::from_bits_truncate(((bits & KIND_MASK) >> 48) as u16),
        )
    }

    pub fn from_parts(index: EntityIndex, gen: Generation, kind: EntityFlags) -> Self {
        let bits = ((index.get()) as u64) | ((gen as u64) << 32) | ((kind.bits() as u64) << 48);

        Self(NonZeroU64::new(bits).unwrap())
    }

    pub fn from_bits(bits: NonZeroU64) -> Self {
        Self(bits)
    }

    pub fn to_bits(&self) -> NonZeroU64 {
        self.0
    }

    /// Construct a static component entity
    pub fn component(index: EntityIndex) -> Entity {
        Self::from_parts(index, 0, EntityFlags::empty())
    }
}

impl Key for Entity {
    fn as_usize(&self) -> usize {
        self.0.get() as _
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

#[derive(Clone, Copy, Debug)]
struct Vacant {
    next: Option<NonZeroU32>,
}

union SlotValue<T> {
    occupied: ManuallyDrop<T>,
    vacant: Vacant,
}

struct Slot<T> {
    val: SlotValue<T>,
    // != 0
    gen: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntityLocation {
    pub(crate) archetype: ArchetypeId,
    pub(crate) slot: usize,
}

pub struct EntityStore {
    slots: Vec<Slot<EntityLocation>>,
    free_head: Option<NonZeroU32>,
}

impl EntityStore {
    pub fn new() -> Self {
        Self::with_capacity(0)
    }
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free_head: None,
        }
    }

    pub fn spawn(&mut self, value: EntityLocation) -> Entity {
        if let Some(index) = self.free_head.take() {
            let free = &self.slot(index).unwrap();
            let gen = free.gen;

            // Update free head
            unsafe {
                self.free_head = free.val.vacant.next;
            }

            Entity::from_parts(index, gen, EntityFlags::empty())
        } else {
            // Push
            let gen = 1;
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                val: SlotValue {
                    occupied: ManuallyDrop::new(value),
                },
                gen,
            });

            Entity::from_parts(
                NonZeroU32::new(index + 1).unwrap(),
                gen,
                EntityFlags::empty(),
            )
        }
    }

    #[inline]
    fn slot(&self, index: EntityIndex) -> Option<&Slot<EntityLocation>> {
        self.slots.get(index.get() as usize - 1)
    }

    #[inline]
    fn slot_mut(&mut self, index: EntityIndex) -> Option<&mut Slot<EntityLocation>> {
        self.slots.get_mut(index.get() as usize - 1)
    }

    pub fn get_mut(&mut self, id: Entity) -> Option<&mut EntityLocation> {
        let (index, gen, _) = id.into_parts();
        let slot = self.slot_mut(index)?;
        if slot.gen == gen {
            Some(unsafe { &mut slot.val.occupied })
        } else {
            None
        }
    }

    pub fn get(&self, id: Entity) -> Option<&EntityLocation> {
        let (index, gen, _) = id.into_parts();
        let slot = self.slot(index)?;
        if slot.gen == gen {
            Some(unsafe { &slot.val.occupied })
        } else {
            None
        }
    }

    pub fn is_alive(&self, id: Entity) -> bool {
        let (index, gen, _) = id.into_parts();
        eprintln!("{index}");
        if let Some(slot) = self.slot(index) {
            slot.gen == gen
        } else {
            false
        }
    }

    pub fn despawn(&mut self, id: Entity) {
        assert!(self.is_alive(id));

        let index = id.index();
        let gen = id.generation();

        let next = self.free_head.take();
        eprintln!("Removing index: {index}");
        let slot = self.slot_mut(index).unwrap();

        eprintln!("id: {id}");
        assert_eq!(slot.gen, gen);
        slot.gen = slot.gen.wrapping_add(1);

        unsafe {
            ManuallyDrop::<EntityLocation>::drop(&mut slot.val.occupied);
        }

        slot.val.vacant = Vacant { next };

        self.free_head = Some(index);
    }
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{
        entity::{EntityFlags, EntityLocation},
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
        let parts = (NonZeroU32::new(0xFFF).unwrap(), 30, EntityFlags::empty());

        let a = Entity::from_parts(parts.0, parts.1, parts.2);

        eprintln!("a: {:b}", a.0.get());

        assert_eq!(parts.0, a.index());
        assert_eq!(parts, a.into_parts());
    }
}
