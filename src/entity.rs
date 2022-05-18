use core::fmt;
use core::num::NonZeroU64;
use std::mem::ManuallyDrop;

use crate::archetype::ArchetypeId;

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
/// The Lower (generation + kind) bit can be ommitted as they do not contribute
/// to uniqueness.
pub struct Entity(NonZeroU64);

const INDEX_MASK: u64 = 0xFFFFFFFE00000000;
const DYNAMIC_MASK: u64 = 0x0000000100000000;
const GENERATION_MASK: u64 = 0x00000000FFFF00;
const KIND_MASK: u64 = 0xFF;

bitflags::bitflags! {
    pub struct EntityKind: u16  {
       const COMPONENT = 1;
    }
}

pub type Dynamic = bool;
pub type Generation = u16;
pub type EntityIndex = u32;

impl Entity {
    pub fn index(self) -> EntityIndex {
        // Can only be constructed from parts
        ((self.0.get() & INDEX_MASK) >> 33) as u32 - 1
    }

    pub fn dynamic(self) -> Dynamic {
        (self.0.get() & DYNAMIC_MASK) != 0
    }

    pub fn generation(self) -> Generation {
        ((self.0.get() & GENERATION_MASK) >> 4) as Generation
    }

    pub fn kind(self) -> EntityKind {
        EntityKind::from_bits_truncate((self.0.get() & KIND_MASK) as u16)
    }

    pub fn into_parts(self) -> (EntityIndex, Dynamic, Generation, EntityKind) {
        let bits = self.0.get();

        (
            ((bits & INDEX_MASK) >> 33) as u32 - 1,
            (bits & DYNAMIC_MASK) != 0,
            ((bits & GENERATION_MASK) >> 16) as Generation,
            EntityKind::from_bits_truncate((bits & KIND_MASK) as u16),
        )
    }

    pub fn from_parts(
        id: EntityIndex,
        dynamic: Dynamic,
        gen: Generation,
        kind: EntityKind,
    ) -> Self {
        let bits = (((id + 1) as u64) << 33)
            | ((dynamic as u64) << 32)
            | ((gen as u64) << 16)
            | (kind.bits() as u64);

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
        let (index, dynamic, generation, kind) = self.into_parts();
        f.debug_tuple("Entity")
            .field(&index)
            .field(&dynamic)
            .field(&generation)
            .field(&kind)
            .finish()
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (index, dynamic, generation, kind) = self.into_parts();
        write!(f, "Entity({index}:{dynamic}:{generation}:{kind:?})")
    }
}

#[derive(Clone, Copy, Debug)]
struct Vacant {
    next: Option<u32>,
}

union SlotValue<T> {
    occupied: ManuallyDrop<T>,
    vacant: Vacant,
}

struct Slot<T> {
    val: SlotValue<T>,
    gen: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntityLocation {
    pub(crate) archetype: ArchetypeId,
    pub(crate) slot: usize,
}

pub struct EntityStore {
    slots: Vec<Slot<EntityLocation>>,
    free_head: Option<u32>,
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
            let free = &self.slot(index);
            let gen = free.gen;

            // Update free head
            unsafe {
                self.free_head = free.val.vacant.next;
            }

            Entity::from_parts(index, true, gen, EntityKind::empty())
        } else {
            // Push
            let gen = 0;
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                val: SlotValue {
                    occupied: ManuallyDrop::new(value),
                },
                gen,
            });

            Entity::from_parts(index, true, gen, EntityKind::empty())
        }
    }

    #[inline]
    fn slot(&self, idx: u32) -> &Slot<EntityLocation> {
        &self.slots[idx as usize]
    }

    #[inline]
    fn slot_mut(&mut self, id: u32) -> &mut Slot<EntityLocation> {
        &mut self.slots[id as usize]
    }

    pub fn get_mut(&mut self, id: Entity) -> Option<&mut EntityLocation> {
        let (index, dynamic, gen, _) = id.into_parts();
        if index < self.slots.len() as _ {
            let slot = self.slot_mut(index);
            if dynamic && slot.gen == gen {
                Some(unsafe { &mut slot.val.occupied })
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn get(&self, id: Entity) -> Option<&EntityLocation> {
        let (index, dynamic, gen, _) = id.into_parts();
        if index < self.slots.len() as _ {
            let slot = self.slot(index);
            if dynamic && slot.gen == gen {
                Some(unsafe { &slot.val.occupied })
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn is_alive(&self, id: Entity) -> bool {
        let (index, dynamic, gen, _) = id.into_parts();
        eprintln!("{index}");
        dynamic && index < self.slots.len() as _ && dbg!(self.slot(index).gen) == gen
    }

    pub fn despawn(&mut self, id: Entity) {
        assert!(self.is_alive(id));

        let index = id.index();
        let gen = id.generation();

        let next = self.free_head.take();
        eprintln!("Removing index: {index}");
        let slot = self.slot_mut(index);

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
        let parts = (0xFFF, true, 30, EntityKind::COMPONENT);

        let a = Entity::from_parts(parts.0, parts.1, parts.2, parts.3);

        eprintln!("a: {:b}", a.0.get());

        assert_eq!(parts.0, a.index());
        assert_eq!(parts, a.into_parts());
    }
}
