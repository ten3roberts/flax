use crate::{archetype::ArchetypeId, error::Result, EntityGen, EntityKind, Error, StrippedEntity};
use std::{iter::Enumerate, mem::ManuallyDrop, num::NonZeroU32, slice};

use super::{Entity, EntityIndex};

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
    // even = dead, odd = alive
    gen: u32,
}

impl<T> Slot<T> {
    pub fn is_alive(&self) -> bool {
        self.gen & 1 == 1
    }
}

pub fn to_slot_gen(gen: EntityGen) -> u32 {
    ((gen as u32) << 1) | 1
}

pub fn from_slot_gen(gen: u32) -> u16 {
    (gen >> 1) as u16
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityLocation {
    pub(crate) slot: usize,
    pub(crate) arch: ArchetypeId,
}

pub struct EntityStore<V = EntityLocation> {
    slots: Vec<Slot<V>>,
    free_head: Option<NonZeroU32>,
    kind: EntityKind,
}

impl<V> EntityStore<V> {
    /// Create a new EntityStore which will spawn entities with a specific kind
    pub fn new(kind: EntityKind) -> Self {
        Self::with_capacity(kind, 0)
    }

    pub fn with_capacity(kind: EntityKind, cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free_head: None,
            kind,
        }
    }

    pub fn spawn(&mut self, value: V) -> Entity {
        if let Some(index) = self.free_head.take() {
            let free = self.slot_mut(index).unwrap();
            debug_assert!(free.gen & 1 == 0);

            free.gen |= 1;

            let gen = from_slot_gen(free.gen);

            // Update free head
            unsafe {
                self.free_head = free.val.vacant.next;
            }

            Entity::from_parts(index, gen, self.kind)
        } else {
            // Push
            let gen = 1;
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                val: SlotValue {
                    occupied: ManuallyDrop::new(value),
                },
                gen: to_slot_gen(gen),
            });

            Entity::from_parts(NonZeroU32::new(index + 1).unwrap(), gen as u16, self.kind)
        }
    }

    #[inline]
    fn slot(&self, index: EntityIndex) -> Option<&Slot<V>> {
        self.slots.get(index.get() as usize - 1)
    }

    #[inline]
    fn slot_mut(&mut self, index: EntityIndex) -> Option<&mut Slot<V>> {
        self.slots.get_mut(index.get() as usize - 1)
    }

    #[inline]
    pub fn get_disjoint(&mut self, a: Entity, b: Entity) -> Option<(&mut V, &mut V)> {
        if a == b {
            return None;
        }

        unsafe {
            let a = &mut *((self.get_mut(a)?) as *mut V);
            let b = &mut *((self.get_mut(b)?) as *mut V);
            Some((a, b))
        }
    }

    #[inline]
    pub fn get_mut(&mut self, id: Entity) -> Option<&mut V> {
        let ns = self.kind;
        assert_eq!(ns, id.kind());

        unsafe {
            self.slot_mut(id.index())
                .filter(|v| v.is_alive())
                .filter(|v| {
                    v.gen == to_slot_gen(id.generation())
                        || id.kind().contains(EntityKind::RELATION)
                })
                .map(|v| &mut *v.val.occupied)
        }
    }

    #[inline]
    pub fn get(&self, id: Entity) -> Option<&V> {
        let ns = self.kind;
        assert_eq!(ns, id.kind());

        unsafe {
            self.slot(id.index())
                .filter(|v| v.is_alive())
                .filter(|v| {
                    v.gen == to_slot_gen(id.generation())
                        || id.kind().contains(EntityKind::RELATION)
                })
                .map(|v| &*v.val.occupied)
        }
    }

    /// Return the entity at a given index
    pub fn at(&self, index: EntityIndex) -> Option<Entity> {
        let slot = self.slot(index)?;

        Some(Entity::from_parts(index, (slot.gen >> 1) as u16, self.kind))
    }

    #[inline]
    pub fn reconstruct(&self, id: StrippedEntity) -> Option<(Entity, &V)> {
        let ns = self.kind;

        assert_eq!(ns, id.kind());

        let slot = self.slot(id.index())?;

        if slot.is_alive() {
            let val = unsafe { &slot.val.occupied };
            Some((id.reconstruct(from_slot_gen(slot.gen)), val))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_alive(&self, id: Entity) -> bool {
        let ns = self.kind;
        assert_eq!(ns, id.kind());

        self.slot(id.index())
            .filter(|v| v.is_alive())
            .filter(|v| {
                v.gen == to_slot_gen(id.generation()) || id.kind().contains(EntityKind::RELATION)
            })
            .is_some()
    }

    pub fn despawn(&mut self, id: Entity) -> Result<()> {
        if !self.is_alive(id) {
            return Err(Error::NoSuchEntity(id));
        }
        eprintln!("Despawning: {id}");

        let index = id.index();

        let next = self.free_head.take();
        let slot = self.slot_mut(index).unwrap();

        slot.gen = slot.gen.wrapping_add(1);

        unsafe {
            ManuallyDrop::<V>::drop(&mut slot.val.occupied);
        }

        slot.val.vacant = Vacant { next };

        self.free_head = Some(index);

        Ok(())
    }

    pub fn iter(&self) -> EntityStoreIter<V> {
        EntityStoreIter {
            iter: self.slots.iter().enumerate(),
            namespace: self.kind,
        }
    }

    /// Spawns an entity at the provided id.
    ///
    /// Fails if the index is occupied.
    pub(crate) fn spawn_at(
        &mut self,
        index: EntityIndex,
        generation: EntityGen,
        value: V,
    ) -> Result<&V> {
        // Init slot
        let free_head = &mut self.free_head;

        eprintln!("Spawning at: {index}");

        let diff = (index.get() as usize).saturating_sub(self.slots.len());

        // Fill the slots between the last and the new with free slots
        // The wanted id may already be inside the valid range, or it may be
        // outside.
        //
        // Regardless, it will now be in the free list
        self.slots.extend(
            (self.slots.len()..)
                .map(|i| {
                    // This slot is not filled so mark it as free
                    let current = NonZeroU32::new(i as u32 + 1).unwrap();
                    // Mark current slot as free
                    let next = free_head.replace(current);

                    Slot {
                        val: SlotValue {
                            vacant: Vacant { next },
                        },
                        gen: 0,
                    }
                })
                .take(diff),
        );

        // Find it in the free list
        let mut prev = None;
        let mut cur = self.free_head;
        while let Some(current) = cur {
            dbg!(current);
            let slot = self.slot(current).expect("Invalid free list node");

            let next_free = unsafe { slot.val.vacant.next };
            assert!(!slot.is_alive());
            if current == index {
                if let Some(prev) = prev {
                    self.slot_mut(prev).unwrap().val.vacant = Vacant { next: next_free }
                } else {
                    self.free_head = next_free;
                }

                let slot = self.slot_mut(current).unwrap();
                *slot = Slot {
                    gen: to_slot_gen(generation),
                    val: SlotValue {
                        occupied: ManuallyDrop::new(value),
                    },
                };

                return unsafe { Ok(&*slot.val.occupied) };
            }

            prev = Some(current);
            cur = next_free
        }

        // It was not free, that means it already exists
        Err(Error::EntityExists(self.at(index).unwrap()))
    }
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new(EntityKind::empty())
    }
}

impl<V> Drop for EntityStore<V> {
    fn drop(&mut self) {
        for slot in &mut self.slots {
            if slot.gen & 1 == 1 {
                unsafe {
                    ManuallyDrop::<V>::drop(&mut slot.val.occupied);
                }
            }
        }
    }
}

pub struct EntityStoreIter<'a, V> {
    iter: Enumerate<slice::Iter<'a, Slot<V>>>,
    namespace: EntityKind,
}

impl<'a, V> Iterator for EntityStoreIter<'a, V> {
    type Item = (Entity, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((index, slot)) = self.iter.next() {
                if slot.gen & 1 == 1 {
                    let val = unsafe { &slot.val.occupied };
                    let id = Entity::from_parts(
                        NonZeroU32::new(index as u32 + 1).unwrap(),
                        (slot.gen >> 1) as u16,
                        self.namespace,
                    );

                    return Some((id, val));
                }
            } else {
                break;
            }
        }

        None
    }
}
