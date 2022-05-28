use crate::{archetype::ArchetypeId, EntityNamespace, Generation};
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

    pub fn get(&self) -> Option<&T> {
        if self.is_alive() {
            Some(unsafe { &self.val.occupied })
        } else {
            None
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.is_alive() {
            Some(unsafe { &mut self.val.occupied })
        } else {
            None
        }
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
    pub(crate) arch: ArchetypeId,
    pub(crate) slot: usize,
}

pub struct EntityStore<V = EntityLocation> {
    slots: Vec<Slot<V>>,
    free_head: Option<NonZeroU32>,
    namespace: EntityNamespace,
}

impl<V> EntityStore<V> {
    pub fn new(namespace: EntityNamespace) -> Self {
        Self::with_capacity(namespace, 0)
    }

    pub fn with_capacity(namespace: EntityNamespace, cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free_head: None,
            namespace,
        }
    }

    pub fn spawn(&mut self, value: V) -> Entity {
        if let Some(index) = self.free_head.take() {
            let free = self.slot_mut(index).unwrap();
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
                .filter(|v| ns == id.namespace() && v.gen == to_slot_gen(id.generation()))?
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
    pub fn is_alive(&self, id: Entity) -> bool {
        dbg!(id.generation(), self.slot(id.index()).unwrap().gen);
        let ns = self.namespace;
        self.slot(id.index())
            .filter(|v| ns == id.namespace() && v.gen == to_slot_gen(id.generation()))
            .is_some()
    }

    pub fn despawn(&mut self, id: Entity) {
        assert!(self.is_alive(id));

        let index = id.index();
        let gen = id.generation();

        let next = self.free_head.take();
        eprintln!("Removing index: {index}");
        let slot = self.slot_mut(index).unwrap();

        eprintln!("id: {id}");
        slot.gen = slot.gen.wrapping_add(1);

        unsafe {
            ManuallyDrop::<V>::drop(&mut slot.val.occupied);
        }

        slot.val.vacant = Vacant { next };

        self.free_head = Some(index);
    }

    pub fn iter(&self) -> EntityStoreIter<V> {
        EntityStoreIter {
            iter: self.slots.iter().enumerate(),
            namespace: self.namespace,
        }
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
    namespace: EntityNamespace,
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
