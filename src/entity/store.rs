use itertools::Itertools;

use super::{Entity, EntityIndex, DEFAULT_GEN};
use crate::{archetype::ArchetypeId, entity::EntityGen, entity::EntityKind, error::Result, Error};
use alloc::vec::Vec;
use core::{
    iter::Enumerate,
    mem::{self, ManuallyDrop},
    ops::Range,
    slice,
    sync::atomic::{AtomicI64, Ordering::Relaxed},
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
    ((gen.get() as u32) << 1) | 1
}

fn from_slot_gen(gen: u32) -> EntityGen {
    EntityGen::new((gen >> 1) as u16).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// An entity's location within an archetype
pub(crate) struct EntityLocation {
    pub(crate) slot: usize,
    pub(crate) arch_id: ArchetypeId,
}

pub(crate) struct EntityStore<V = EntityLocation> {
    slots: Vec<Slot<V>>,
    free: Vec<EntityIndex>,
    pub(crate) kind: EntityKind,
    /// Increases for each freed item
    /// Decreases for each reserved id
    ///
    /// If there are more reserved ids than free, the value is negative and indicates that ids are
    /// taken from not yet allocated slots.
    cursor: AtomicI64,
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

    pub fn reserve_one(&self) -> Entity {
        let cursor = self.cursor.fetch_sub(1, Relaxed);

        if cursor > 0 {
            let index = self.free[cursor as usize - 1];
            let gen = from_slot_gen(self.slot(index).unwrap().gen);
            Entity::from_parts(index, gen, self.kind)
        } else {
            let next_slot = (self.slots.len() + (-cursor).max(0) as usize) as u32;
            Entity::from_parts(next_slot, DEFAULT_GEN, self.kind)
        }
    }
    /// Reserves `count` new entity ids
    pub fn reserve(&self, count: usize) -> ReservedIter<V> {
        // Use as many free as possible
        let cursor = self.cursor.fetch_sub(count as _, Relaxed);

        // ----------------------------------
        // | free list             | cursor |
        // ----------------------------------
        let free = &self.free[(cursor - count as i64).max(0) as usize..cursor.max(0) as usize];
        let next_slot = (self.slots.len() + (-cursor).max(0) as usize) as u32;

        let new = next_slot..next_slot + (count as i64 - cursor.max(0)).max(0) as u32;

        ReservedIter {
            slots: &self.slots,
            free: free.iter(),
            new,
            kind: self.kind,
        }
    }

    /// Converts all reserved ids into actual entities using the provided values
    pub fn flush_reserved(&mut self, mut acquire: impl FnMut(Entity) -> V) {
        let cursor = self.cursor.load(Relaxed);
        let free = &self.free[(cursor.max(0) as usize)..self.free.len()];

        for &index in free {
            let slot = &mut self.slots[index as usize];
            let gen = from_slot_gen(slot.gen);
            let id = Entity::from_parts(index, gen, self.kind);

            slot.make_alive(acquire(id));
        }

        self.len += (self.free.len() as i64 - cursor) as usize;
        self.free.truncate(cursor.max(0) as usize);

        let next_slot = self.slots.len() as u32;
        let new_count = (-cursor).max(0) as usize;
        let new = next_slot..next_slot + new_count as u32;

        self.slots.reserve(new_count);

        for index in new {
            let id = Entity::from_parts(index, DEFAULT_GEN, self.kind);

            self.slots.push(Slot {
                value: SlotValue {
                    occupied: ManuallyDrop::new(acquire(id)),
                },
                gen: to_slot_gen(DEFAULT_GEN),
            });
        }

        self.cursor.store(self.free.len() as _, Relaxed);
    }

    pub fn with_capacity(kind: EntityKind, cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free: Vec::new(),
            kind,
            len: 0,
            cursor: AtomicI64::new(0),
        }
    }

    #[inline]
    fn assert_reserved(&self) {
        #[cfg(debug_assertions)]
        if self.cursor.load(Relaxed) != self.free.len() as i64 {
            panic!("Attempt to spawn while there are allocated ids");
        }
    }

    pub fn spawn(&mut self, value: V) -> Entity {
        self.assert_reserved();

        if let Some(index) = self.free.pop() {
            self.cursor.fetch_sub(1, Relaxed);

            let slot = &mut self.slots[index as usize];
            debug_assert!(!slot.is_alive());

            // Make the slot generation odd again which means this slot is
            // alive.
            let (gen, _) = slot.make_alive(value);

            let id = Entity::from_parts(index, gen, self.kind);
            self.len += 1;
            id
        } else {
            // Push
            let index = self.slots.len() as u32;

            self.slots.push(Slot {
                value: SlotValue {
                    occupied: ManuallyDrop::new(value),
                },
                gen: to_slot_gen(DEFAULT_GEN),
            });

            self.len += 1;
            Entity::from_parts(index, DEFAULT_GEN, self.kind)
        }
    }

    pub fn despawn(&mut self, id: Entity) -> Result<V> {
        self.assert_reserved();
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
        self.cursor.fetch_add(1, Relaxed);

        self.len -= 1;

        Ok(val)
    }

    #[inline]
    fn slot(&self, index: EntityIndex) -> Option<&Slot<V>> {
        self.slots.get(index as usize)
    }

    #[inline]
    fn slot_mut(&mut self, index: EntityIndex) -> Option<&mut Slot<V>> {
        self.slots.get_mut(index as usize)
    }

    #[inline]
    pub(crate) fn get_disjoint(&mut self, a: Entity, b: Entity) -> Option<(&mut V, &mut V)> {
        if a == b || !self.is_alive(a) || !self.is_alive(b) {
            return None;
        }

        unsafe {
            let base = self.slots.as_mut_ptr();
            let a = base.add(a.index() as usize);
            let b = base.add(b.index() as usize);

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

    /// Ensures an entity will not spawn at this id
    pub(crate) fn reserve_at(&mut self, index: EntityIndex) -> crate::error::Result<()> {
        self.assert_reserved();
        self.take_slot(index)?;

        self.len += 1;
        let slot = self.slot_mut(index).unwrap();

        debug_assert!(!slot.is_alive());

        Ok(())
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
        self.take_slot(index)?;
        self.len += 1;
        let slot = self.slot_mut(index).unwrap();

        debug_assert!(!slot.is_alive());

        slot.gen = to_slot_gen(gen);
        slot.value = SlotValue {
            occupied: ManuallyDrop::new(value),
        };

        Ok(unsafe { &mut slot.value.occupied })
    }

    fn take_slot(&mut self, index: EntityIndex) -> Result<()> {
        self.assert_reserved();
        if index as usize >= self.slots.len() {
            // The current slot does not exist
            let new_free = self.slots.len() as u32..index;
            self.cursor.fetch_add(new_free.len() as _, Relaxed);

            self.free.extend(new_free);

            self.slots.resize_with(index as usize + 1, || Slot {
                value: SlotValue { vacant: Vacant },
                gen: 2,
            });
        } else if let Some(pos) = self.free.iter().position(|&v| v == index) {
            self.cursor.fetch_sub(1, Relaxed);
            self.free.swap_remove(pos);
        } else if let Some((id, _)) = self.reconstruct(index) {
            return Err(Error::EntityOccupied(id));
        } else {
            // reserve_at
        };

        Ok(())
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
                let id = Entity::from_parts(index as u32, from_slot_gen(slot.gen), self.namespace);

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
                let id = Entity::from_parts(index as u32, from_slot_gen(slot.gen), self.namespace);

                return Some((id, val));
            }
        }

        None
    }
}

/// Iterates upon newly reserved entity ids
pub(crate) struct ReservedIter<'a, V = EntityLocation> {
    slots: &'a [Slot<V>],
    free: slice::Iter<'a, EntityIndex>,
    new: Range<EntityIndex>,
    kind: EntityKind,
}

impl<'a, V> ExactSizeIterator for ReservedIter<'a, V> {
    fn len(&self) -> usize {
        self.free.len() + self.new.len()
    }
}

impl<'a, V> Iterator for ReservedIter<'a, V> {
    type Item = Entity;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(&index) = self.free.next() {
            // The gen as if alive
            let slot = &self.slots[index as usize];
            let gen = from_slot_gen(slot.gen);
            Some(Entity::from_parts(index, gen, self.kind))
        } else if let Some(index) = self.new.next() {
            Some(Entity::from_parts(index, DEFAULT_GEN, self.kind))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

#[cfg(test)]
mod test {

    use alloc::collections::BTreeMap;

    use super::*;

    #[test]
    fn spawn_at() {
        let mut store = EntityStore::new(EntityKind::empty());
        let a = store.spawn("a");
        let b = store.spawn("b");
        store.despawn(a).unwrap();
        let c = store.spawn("c");
        assert_eq!(a.index(), c.index());
        assert_eq!(b.gen(), DEFAULT_GEN);
        assert!(!store.is_alive(a));
        assert_eq!(c.gen(), DEFAULT_GEN.saturating_add(1));

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

        let d_val = store.spawn_at(9, DEFAULT_GEN, "d").unwrap();
        assert_eq!(*d_val, "d");

        let slot = store.slot(9).unwrap();
        assert!(slot.is_alive());

        let slot = store.slot(2).unwrap();
        assert!(!slot.is_alive());

        let slot = store.slot(4).unwrap();
        assert!(!slot.is_alive());

        let slot = store.slot(7).unwrap();
        assert!(!slot.is_alive());

        let slot = store.slot(1).unwrap();
        assert!(slot.is_alive());

        unsafe {
            assert_eq!(*slot.value.occupied, "b");
        }

        let slot = store.slot(9).unwrap();
        assert!(slot.is_alive());

        unsafe {
            assert_eq!(*slot.value.occupied, "d");
        }

        let e = store.spawn("e");
        assert_eq!(e.index(), 8);
        assert!(!store.is_alive(long_dead));

        store
            .spawn_at(long_dead.index(), long_dead.gen(), "long_dead")
            .unwrap();

        store.spawn_at(4, DEFAULT_GEN, "reserved").unwrap();
    }

    #[test]
    fn reserve_one() {
        let mut store = EntityStore::new(EntityKind::empty());
        let a = store.spawn("a");
        let b = store.spawn("b");

        let c = store.reserve_one();
        assert_eq!(c, Entity::from_parts(2, DEFAULT_GEN, EntityKind::empty()));

        store.flush_reserved(|id| {
            assert_eq!(id, c);
            "c"
        });

        assert_eq!(store.get(a), Some(&"a"));
        assert_eq!(store.get(b), Some(&"b"));
        assert_eq!(store.get(c), Some(&"c"));
    }

    #[test]
    fn reserve_many() {
        let mut store = EntityStore::new(EntityKind::empty());
        let a = store.spawn("a");
        let b = store.spawn("b");
        let _ = store.spawn("_");
        store.despawn(b).unwrap();

        let r = Entity::from_parts(8, DEFAULT_GEN.saturating_add(1), EntityKind::empty());
        store.reserve_at(r.index()).unwrap();

        let mut ids = store.reserve(2).collect_vec();

        ids.extend(store.reserve(3));
        ids.push(store.reserve_one());

        let create_id = |i, g| Entity::from_parts(i, g, EntityKind::empty());

        let expected = [
            (create_id(6, DEFAULT_GEN), "g"),
            (create_id(7, DEFAULT_GEN), "h"),
            (create_id(3, DEFAULT_GEN), "d"),
            (create_id(4, DEFAULT_GEN), "e"),
            (create_id(5, DEFAULT_GEN), "f"),
            (create_id(1, DEFAULT_GEN.saturating_add(1)), "c"),
        ];

        assert_eq!(ids, expected.iter().map(|v| v.0).collect_vec());

        let mut e: BTreeMap<_, _> = expected.into_iter().collect();
        store.flush_reserved(|id| e.remove(&id).unwrap());

        assert!(!store.is_alive(r));
        store
            .spawn_at(8, DEFAULT_GEN.saturating_add(1), "r")
            .unwrap();

        assert!(store.is_alive(r));

        assert_eq!(store.get(a), Some(&"a"));
        for expected in expected {
            assert_eq!(store.get(expected.0), Some(&expected.1));
        }
    }
}
