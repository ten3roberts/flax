use core::fmt;
use core::num::{NonZeroU32, NonZeroU64};

#[derive(Clone, Copy, PartialEq)]
#[repr(transparent)]
pub struct Entity(NonZeroU64);

const ID_MASK: u64 = 0xFFFF0000;

impl Entity {
    fn id(&self) -> u32 {
        self.0.get() as u32
    }

    pub fn gen(&self) -> u32 {
        (self.0.get() >> 32) as u32
    }

    pub fn into_parts(&self) -> (NonZeroU32, u32) {
        let num = self.0.get();
        unsafe { (NonZeroU32::new_unchecked(num as u32), (num >> 32) as u32) }
    }

    pub fn from_parts(id: NonZeroU32, gen: u32) -> Self {
        Self(NonZeroU64::new(id.get() as u64 | (gen as u64) << 32).unwrap())
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
        f.debug_tuple("Entity")
            .field(&self.id())
            .field(&self.gen())
            .finish()
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}:{})", self.id(), self.gen())
    }
}

#[derive(Clone, Copy, Debug)]
struct Vacant {
    next: Option<NonZeroU32>,
}

union SlotValue {
    occupied: (),
    vacant: Vacant,
}

struct Slot {
    val: SlotValue,
    gen: u32,
}

pub struct EntityStore {
    slots: Vec<Slot>,
    count: u32,
    cap: usize,
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
            count: 0,
            cap,
        }
    }

    pub fn spawn(&mut self) -> Entity {
        if let Some(idx) = self.free_head.take() {
            let free = &self.slot(idx);
            let gen = free.gen;

            // Update free head
            unsafe {
                self.free_head = free.val.vacant.next;
            }

            Entity::from_parts(idx, gen)
        } else {
            // Push
            let gen = 0;
            self.slots.push(Slot {
                val: SlotValue { occupied: () },
                gen,
            });
            let idx = self.slots.len();

            Entity::from_parts(NonZeroU32::new(idx as u32).unwrap(), gen)
        }
    }

    fn slot(&self, idx: NonZeroU32) -> &Slot {
        &self.slots[idx.get() as usize - 1]
    }

    fn get_mut(&mut self, id: NonZeroU32) -> &mut Slot {
        &mut self.slots[id.get() as usize - 1]
    }

    pub fn is_alive(&self, id: Entity) -> bool {
        eprintln!("{id}");
        let (id, gen) = id.into_parts();
        id.get() <= self.slots.len() as _ && self.slot(id).gen == gen
    }

    pub fn despawn(&mut self, id: Entity) {
        assert!(self.is_alive(id));

        let (id, gen) = id.into_parts();

        let next = self.free_head.take();
        let slot = self.get_mut(id);

        assert_eq!(slot.gen, gen);
        slot.gen = slot.gen.wrapping_add(1);

        slot.val.vacant = Vacant { next };

        self.free_head = Some(id);
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

    use crate::Entity;

    use super::EntityStore;
    #[test]
    fn entity_store() {
        let mut entities = EntityStore::new();
        let a = entities.spawn();
        let b = entities.spawn();
        let c = entities.spawn();

        entities.despawn(b);

        assert!(entities.is_alive(a));
        assert!(!entities.is_alive(b));
        assert!(entities.is_alive(c));
    }

    #[test]
    fn entity_id() {
        let parts = (NonZeroU32::new(1939).unwrap(), 239);
        let a = Entity::from_parts(parts.0, parts.1);
        assert_eq!(parts, a.into_parts());
    }
}
