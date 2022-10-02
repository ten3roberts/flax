use itertools::Itertools;

use super::{Entity, EntityIndex};
use crate::{archetype::ArchetypeId, entity::EntityGen, entity::EntityKind, error::Result, Error};
use alloc::vec::Vec;
use core::{
    iter::Enumerate,
    mem::{self, ManuallyDrop},
    num::NonZeroU32,
    slice,
    sync::atomic::AtomicU32,
};

#[derive(Clone, Copy, Debug)]
struct Vacant;

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

    pub fn make_alive(&mut self, value: T) -> (EntityGen, &mut T) {
        debug_assert!(!self.is_alive());

        // Make the slot generation odd again which means this slot is
        // alive.
        self.gen |= 1;
        self.value = SlotValue {
            occupied: ManuallyDrop::new(value),
        };

        (from_slot_gen(self.gen), unsafe { &mut self.value.occupied })
    }

    fn make_dead(&mut self) -> T {
        debug_assert!(self.is_alive());

        let val = mem::replace(&mut self.value, SlotValue { vacant: Vacant });
        let val = unsafe { ManuallyDrop::<T>::into_inner(val.occupied) };

        // Since the slot is alive, the gen is odd, adding one makes it even
        self.gen = self.gen.wrapping_add(1);
        val
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
    free: Vec<EntityIndex>,
    pub(crate) kind: EntityKind,
    /// A positive value indicates the number of entities which are reserved from the free list
    cursor: AtomicU32,
    len: usize,
}

impl<V> core::fmt::Debug for EntityStore<V>
where
    V: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

    /// Reserves `count` slots and returns ids which can be safely spawned at
    pub fn reserve(&self, count: u32) {
        todo!()
    }

    // /// Removes a slot from the free list
    // fn unlink_free(&mut self, index: EntityIndex) -> Result<&mut Slot<V>> {
    //     if let Some((prev, index, slot)) = self.free_mut().find(|v| v.1 == index) {
    //         let next_free = unsafe { slot.value.vacant.next };
    //         self.len += 1;

    //         if let Some(prev) = prev {
    //             self.slot_mut(prev).unwrap().value.vacant = Vacant { next: next_free }
    //         } else {
    //             self.free_head = next_free;
    //         }

    //         Ok(self.slot_mut(index).unwrap())
    //     } else {
    //         let kind = self.kind;
    //         let slot = self.slot_mut(index).unwrap();

    //         Err(Error::EntityOccupied(Entity::from_parts(
    //             index,
    //             from_slot_gen(slot.gen),
    //             kind,
    //         )))
    //     }
    // }

    // fn free_mut(&mut self) -> FreeIterMut<V> {
    //     let cur = self.free_head;
    //     FreeIterMut {
    //         store: self,
    //         cur,
    //         prev: None,
    //     }
    // }

    // fn free(&mut self) -> FreeIter<V> {
    //     let cur = self.free_head;
    //     FreeIter {
    //         store: self,
    //         cur,
    //         prev: None,
    //     }
    // }

    pub fn with_capacity(kind: EntityKind, cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free: Vec::new(),
            kind,
            len: 0,
            cursor: AtomicU32::new(0),
        }
    }

    pub fn spawn(&mut self, value: V) -> Entity {
        if let Some(index) = self.free.pop() {
            let slot = { self.slots.get_mut(index.get() as usize - 1) }.unwrap();
            debug_assert!(slot.gen & 1 == 0);

            // Make the slot generation odd again which means this slot is
            // alive.
            let (gen, _) = slot.make_alive(value);

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
            Entity::from_parts(NonZeroU32::new(index + 1).unwrap(), gen, self.kind)
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
                .filter(|v| v.is_alive() && v.gen == to_slot_gen(id.gen()))
                .map(|v| &mut *v.value.occupied)
        }
    }

    #[inline]
    pub fn get(&self, id: Entity) -> Option<&V> {
        let ns = self.kind;
        assert_eq!(ns, id.kind());

        unsafe {
            let val = self.slot(id.index());

            // let val = val.filter(|v| v.is_alive()).filter(|v| {
            //     v.gen == to_slot_gen(id.generation()) || id.kind().contains(EntityKind::RELATION)
            // })?;
            let val = val.filter(|v| v.is_alive() && v.gen == to_slot_gen(id.gen()))?;

            let val = &val.value.occupied;

            Some(val)
        }
    }

    #[inline]
    pub fn reconstruct(&self, index: EntityIndex) -> Option<(Entity, &V)> {
        let slot = self.slot(index)?;

        if slot.is_alive() {
            let val = unsafe { &slot.value.occupied };
            Some((
                Entity::from_parts(index, from_slot_gen(slot.gen), self.kind),
                val,
            ))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_alive(&self, id: Entity) -> bool {
        let ns = self.kind;
        assert_eq!(ns, id.kind());

        self.slot(id.index())
            .filter(|v| v.is_alive() && v.gen == to_slot_gen(id.gen()))
            .is_some()
    }

    pub fn despawn(&mut self, id: Entity) -> Result<V> {
        if !self.is_alive(id) {
            return Err(Error::NoSuchEntity(id));
        }

        let index = id.index();

        let kind = self.kind;
        let slot = self.slot_mut(index).unwrap();

        // Make sure static ids never get a generation
        if kind.contains(EntityKind::STATIC) {
            panic!("Attempt to despawn static entity");
        }

        let val = slot.make_dead();
        self.free.push(index);

        self.len -= 1;

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
        gen: EntityGen,
        value: V,
    ) -> crate::error::Result<&mut V> {
        if index.get() as usize > self.slots.len() {
            // The current slot does not exist
            self.free.extend(
                (self.slots.len() as u32 + 1..index.get() as u32)
                    .map(|v| NonZeroU32::new(v).unwrap()),
            );

            self.slots.resize_with(index.get() as usize, || Slot {
                value: SlotValue { vacant: Vacant },
                gen: 0,
            });
        } else if let Some(pos) = self.free.iter().position(|&v| v == index) {
            self.free.swap_remove(pos);
        } else {
            let id = self.reconstruct(index).unwrap().0;
            return Err(Error::EntityOccupied(id));
        };

        self.len += 1;
        let slot = self.slot_mut(index).unwrap();

        debug_assert!(!slot.is_alive());

        slot.gen = to_slot_gen(gen);
        slot.value = SlotValue {
            occupied: ManuallyDrop::new(value),
        };

        Ok(unsafe { &mut slot.value.occupied })
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

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn spawn_at() {
        let mut store = EntityStore::new(EntityKind::empty());
        let a = store.spawn("a");
        let b = store.spawn("b");
        store.despawn(a).unwrap();
        let c = store.spawn("c");
        assert_eq!(a.index(), c.index());
        assert_eq!(b.gen(), 1);
        assert!(!store.is_alive(a));
        assert_eq!(c.gen(), 2);

        let long_dead = store.spawn("long_dead");
        store.despawn(long_dead).unwrap();

        assert_eq!(store.get(c), Some(&"c"));
        store.despawn(c).unwrap();

        let a_val = store.spawn_at(a.index(), a.gen(), "a").unwrap();
        assert_eq!(*a_val, "a");

        assert_eq!(
            store.spawn_at(a.index(), a.gen(), "_"),
            Err(Error::EntityOccupied(a))
        );

        let d_val = store
            .spawn_at(EntityIndex::new(9).unwrap(), 1, "d")
            .unwrap();
        assert_eq!(*d_val, "d");

        let slot = store.slot(EntityIndex::new(3).unwrap()).unwrap();
        assert!(!slot.is_alive());

        let slot = store.slot(EntityIndex::new(5).unwrap()).unwrap();
        assert!(!slot.is_alive());

        let slot = store.slot(EntityIndex::new(8).unwrap()).unwrap();
        assert!(!slot.is_alive());

        let slot = store.slot(EntityIndex::new(2).unwrap()).unwrap();
        assert!(slot.is_alive());

        unsafe {
            assert_eq!(*slot.value.occupied, "b");
        }

        let slot = store.slot(EntityIndex::new(9).unwrap()).unwrap();
        assert!(slot.is_alive());

        unsafe {
            assert_eq!(*slot.value.occupied, "d");
        }

        let e = store.spawn("e");
        assert_eq!(e.index(), NonZeroU32::new(8).unwrap());
        assert!(!store.is_alive(long_dead));

        dbg!(long_dead);
        store
            .spawn_at(long_dead.index(), long_dead.gen(), "long_dead")
            .unwrap();

        store
            .spawn_at(EntityIndex::new(5).unwrap(), 1, "reserved")
            .unwrap();
    }
}
