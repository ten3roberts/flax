use itertools::Itertools;

use crate::{
    archetype::ArchetypeId, entity::EntityGen, entity::EntityKind, entity::StrippedEntity,
    error::Result, Error,
};
use std::{
    iter::Enumerate,
    mem::{self, ManuallyDrop},
    num::NonZeroU32,
    slice,
};

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
    value: SlotValue<T>,
    // even = dead, odd = alive
    gen: u32,
}

impl<T> Slot<T> {
    pub fn is_alive(&self) -> bool {
        self.gen & 1 == 1
    }
}

fn to_slot_gen(gen: EntityGen) -> u32 {
    ((gen as u32) << 1) | 1
}

fn from_slot_gen(gen: u32) -> u16 {
    (gen >> 1) as u16
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// An entity's location within an archetype
pub(crate) struct EntityLocation {
    pub(crate) slot: usize,
    pub(crate) arch: ArchetypeId,
}

pub(crate) struct EntityStore<V = EntityLocation> {
    slots: Vec<Slot<V>>,
    free_head: Option<NonZeroU32>,
    pub(crate) kind: EntityKind,
    len: usize,
}

impl<V> std::fmt::Debug for EntityStore<V>
where
    V: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntityStore")
            .field(
                "slots",
                &self
                    .slots
                    .iter()
                    .filter(|v| v.is_alive())
                    .map(|v| unsafe { &*v.value.occupied })
                    .collect_vec(),
            )
            .field("free_head", &self.free_head)
            .field("kind", &self.kind)
            .field("len", &self.len)
            .finish()
    }
}

impl<'a, V> IntoIterator for &'a EntityStore<V> {
    type Item = (Entity, &'a V);

    type IntoIter = EntityStoreIter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, V> IntoIterator for &'a mut EntityStore<V> {
    type Item = (Entity, &'a mut V);

    type IntoIter = EntityStoreIterMut<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
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
            len: 0,
        }
    }

    pub fn spawn(&mut self, value: V) -> Entity {
        if let Some(index) = self.free_head.take() {
            let slot = { self.slots.get_mut(index.get() as usize - 1) }.unwrap();
            debug_assert!(slot.gen & 1 == 0);

            // Update free head
            unsafe {
                self.free_head = slot.value.vacant.next;
            }

            // Make the slot generation odd again which means this slot is
            // alive.
            slot.gen |= 1;
            slot.value = SlotValue {
                occupied: ManuallyDrop::new(value),
            };

            let gen = from_slot_gen(slot.gen);

            let id = Entity::from_parts(index, gen, self.kind);
            self.len += 1;
            id
        } else {
            // Push
            let gen = 1;
            let index = self.slots.len() as u32;

            self.slots.push(Slot {
                value: SlotValue {
                    occupied: ManuallyDrop::new(value),
                },
                gen: to_slot_gen(gen),
            });

            self.len += 1;
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
    pub(crate) fn get_disjoint(&mut self, a: Entity, b: Entity) -> Option<(&mut V, &mut V)> {
        if a == b || !self.is_alive(a) || !self.is_alive(b) {
            return None;
        }

        unsafe {
            let base = self.slots.as_mut_ptr();
            let a = base.add(a.index().get() as usize - 1);
            let b = base.add(b.index().get() as usize - 1);

            assert_ne!(a, b);
            let a = &mut (*a).value.occupied;
            let b = &mut (*b).value.occupied;

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
                .map(|v| &mut *v.value.occupied)
        }
    }

    #[inline]
    pub fn get(&self, id: Entity) -> Option<&V> {
        let ns = self.kind;
        assert_eq!(ns, id.kind());

        unsafe {
            let val = self.slot(id.index());

            let val = val.filter(|v| v.is_alive()).filter(|v| {
                v.gen == to_slot_gen(id.generation()) || id.kind().contains(EntityKind::RELATION)
            })?;

            let val = &val.value.occupied;

            Some(val)
        }
    }

    #[inline]
    pub fn reconstruct(&self, id: StrippedEntity) -> Option<(Entity, &V)> {
        let ns = self.kind;

        assert_eq!(ns, id.kind());

        let slot = self.slot(id.index())?;

        if slot.is_alive() {
            let val = unsafe { &slot.value.occupied };
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

    pub fn despawn(&mut self, id: Entity) -> Result<V> {
        if !self.is_alive(id) {
            return Err(Error::NoSuchEntity(id));
        }

        let index = id.index();

        let next = self.free_head.take();
        let kind = self.kind;
        let slot = self.slot_mut(index).unwrap();

        // Make sure static ids never get a generation
        if kind.contains(EntityKind::STATIC) {
            slot.gen = 0b10
        } else {
            slot.gen = slot.gen.wrapping_add(1);
        }

        let inner = mem::replace(
            &mut slot.value,
            SlotValue {
                vacant: Vacant { next },
            },
        );
        let val = unsafe { ManuallyDrop::<V>::into_inner(inner.occupied) };

        slot.value.vacant = Vacant { next };

        self.free_head = Some(index);
        self.len += 1;

        Ok(val)
    }

    pub fn iter(&self) -> EntityStoreIter<V> {
        EntityStoreIter {
            iter: self.slots.iter().enumerate(),
            namespace: self.kind,
        }
    }

    pub fn iter_mut(&mut self) -> EntityStoreIterMut<V> {
        EntityStoreIterMut {
            iter: self.slots.iter_mut().enumerate(),
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
    ) -> Option<&mut V> {
        // Init slot
        let free_head = &mut self.free_head;

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
                        value: SlotValue {
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
            let slot = self.slot(current).expect("Invalid free list node");

            let next_free = unsafe { slot.value.vacant.next };
            assert!(!slot.is_alive());
            if current == index {
                self.len += 1;

                if let Some(prev) = prev {
                    self.slot_mut(prev).unwrap().value.vacant = Vacant { next: next_free }
                } else {
                    self.free_head = next_free;
                }

                let slot = self.slot_mut(current).unwrap();
                *slot = Slot {
                    gen: to_slot_gen(generation),
                    value: SlotValue {
                        occupied: ManuallyDrop::new(value),
                    },
                };

                return unsafe { Some(&mut *slot.value.occupied) };
            }

            prev = Some(current);
            cur = next_free
        }

        // It was not free, that means it already exists
        None
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
            if slot.is_alive() {
                unsafe {
                    ManuallyDrop::<V>::drop(&mut slot.value.occupied);
                }
            }
        }
    }
}

pub(crate) struct EntityStoreIter<'a, V> {
    iter: Enumerate<slice::Iter<'a, Slot<V>>>,
    namespace: EntityKind,
}

impl<'a, V> Iterator for EntityStoreIter<'a, V> {
    type Item = (Entity, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        for (index, slot) in self.iter.by_ref() {
            if slot.is_alive() {
                let val = unsafe { &slot.value.occupied };
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

pub(crate) struct EntityStoreIterMut<'a, V> {
    iter: Enumerate<slice::IterMut<'a, Slot<V>>>,
    namespace: EntityKind,
}

impl<'a, V> Iterator for EntityStoreIterMut<'a, V> {
    type Item = (Entity, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        for (index, slot) in self.iter.by_ref() {
            if slot.is_alive() {
                let val = unsafe { &mut slot.value.occupied };
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
