use crate::{archetype::ArchetypeId, error::Result, Error, Generation, Namespace, StrippedEntity};
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

pub fn to_slot_gen(gen: Generation) -> u32 {
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
    namespace: Namespace,
}

impl<V> EntityStore<V> {
    pub fn new(namespace: Namespace) -> Self {
        Self::with_capacity(namespace, 0)
    }

    pub fn with_capacity(namespace: Namespace, cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free_head: None,
            namespace,
        }
    }

    pub fn spawn(&mut self, value: V) -> Entity {
        if let Some(index) = self.free_head.take() {
            let free = self.slot_mut(index).unwrap();
            assert!(free.gen & 1 == 0);
            free.gen = free.gen | 1;
            let gen = from_slot_gen(free.gen);

            // Update free head
            unsafe {
                self.free_head = free.val.vacant.next;
            }

            Entity::from_parts(index, gen, self.namespace)
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

            Entity::from_parts(
                NonZeroU32::new(index + 1).unwrap(),
                gen as u16,
                self.namespace,
            )
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
            let a = &mut *((self.get(a)?) as *const V as *mut V);
            let b = &mut *((self.get(b)?) as *const V as *mut V);
            Some((a, b))
        }
    }

    #[inline]
    pub fn get_mut(&mut self, id: Entity) -> Option<&mut V> {
        let ns = self.namespace;
        Some(unsafe {
            &mut self
                .slot_mut(id.index())
                .filter(|v| ns == id.namespace() && (v.gen == to_slot_gen(id.generation())))?
                .val
                .occupied
        })
    }

    #[inline]
    pub fn get(&self, id: Entity) -> Option<&V> {
        let ns = self.namespace;
        Some(unsafe {
            &self
                .slot(id.index())
                .filter(|v| ns == id.namespace() && v.gen == to_slot_gen(id.generation()))?
                .val
                .occupied
        })
    }

    #[inline]
    pub fn reconstruct(&self, id: StrippedEntity) -> Option<(Entity, &V)> {
        let ns = self.namespace;

        assert_eq!(ns, id.namespace());

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
        let ns = self.namespace;
        self.slot(id.index())
            .filter(|v| ns == id.namespace() && v.gen == to_slot_gen(id.generation()))
            .is_some()
    }

    pub fn despawn(&mut self, id: Entity) -> Result<()> {
        if !self.is_alive(id) {
            return Err(Error::NoSuchEntity(id));
        }
        eprintln!("Despawning: {id} in {}", self.namespace);

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
            namespace: self.namespace,
        }
    }

    /// Spawns an entity at the provided id.
    /// Any entity with the same index as id will be despawned
    pub(crate) fn spawn_at(&mut self, id: Entity, value: V) -> &V {
        dbg!(id);
        let ns = self.namespace;
        assert_eq!(ns, id.namespace());

        let index = id.index();

        // Init slot
        let free_head = &mut self.free_head;
        dbg!(self.slots.len(), index.get(), *free_head, self.namespace);

        let diff = (index.get() as usize).saturating_sub(self.slots.len());

        eprintln!("Padding with: {diff}");
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
                    eprintln!("Replacing free head {free_head:?} with {current}");
                    // Mark current slot as free
                    let next = free_head.replace(current);

                    eprintln!("Next: {next:?}");
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
                eprintln!("Found slot in free list: {id}");
                if let Some(prev) = prev {
                    self.slot_mut(prev).unwrap().val.vacant = Vacant { next: next_free }
                } else {
                    self.free_head = next_free;
                }

                let slot = self.slot_mut(current).unwrap();
                *slot = Slot {
                    gen: to_slot_gen(id.generation()),
                    val: SlotValue {
                        occupied: ManuallyDrop::new(value),
                    },
                };

                return unsafe { &*slot.val.occupied };
            }

            // let next_free = unsafe { slot.val.vacant.next };
            // if let Some(next) = next {
            //     if next == index {
            //         let next_slot = self.slot(next).unwrap();
            //         // Oh no
            //         eprintln!("Found id {id} in free list");
            //         self.slot_mut(current).unwrap().val.vacant.next =
            //             unsafe { next_slot.val.vacant.next };
            //     }
            // }

            prev = Some(current);
            cur = next_free
        }
        eprintln!("Slot is alive");

        // It was not free, that means it already exists
        let slot = self.slot_mut(index).expect("Padded vector");

        assert!(slot.is_alive());

        eprintln!("Init {id}");

        unsafe {
            ManuallyDrop::<V>::drop(&mut slot.val.occupied);
        }

        *slot = Slot {
            gen: to_slot_gen(id.generation()),
            val: SlotValue {
                occupied: ManuallyDrop::new(value),
            },
        };

        unsafe { &*slot.val.occupied }
    }
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new(1)
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
    namespace: Namespace,
}

impl<'a, V> Iterator for EntityStoreIter<'a, V> {
    type Item = (Entity, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, slot)) = self.iter.next() {
            if slot.gen & 1 == 1 {
                let val = unsafe { &slot.val.occupied };
                let id = Entity::from_parts(
                    NonZeroU32::new(index as u32 + 1).unwrap(),
                    (slot.gen >> 1) as u16,
                    self.namespace,
                );

                return Some((id, val));
            }
        }

        None
    }
}
