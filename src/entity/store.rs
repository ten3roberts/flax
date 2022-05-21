use crate::archetype::ArchetypeId;
use std::{cmp::Ordering, collections::BTreeMap, mem::ManuallyDrop, num::NonZeroU32};

use super::{Entity, EntityIndex, EntityKind};

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

            Entity::from_parts(index, gen, EntityKind::empty())
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
                EntityKind::empty(),
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

// /// A map implementation for associating extra local data to an
// /// entity.
// pub struct EntityMap<V> {
//     inner: Inner<V>,
// }

// enum Inner<V> {
//     Linear(Vec<(Entity, V)>),
//     Tree(BTreeMap<Entity, V>),
// }

// impl<V> Inner<V> {
//     pub fn new() -> Self {
//         Self::Linear(Vec::new())
//     }

//     pub fn insert(&mut self, entity: Entity, value: V) {
//         match self {
//             Inner::Linear(data) => {
//                 let mut l = 0;
//                 let mut r = data.len();

//                 loop {
//                     let i = (r - l) / 2;
//                     let (mid, v) = &mut data[i];
//                     match entity.cmp(mid) {
//                         Ordering::Less => r = i,
//                         Ordering::Equal => {
//                             // Replace
//                             *v = value;
//                             return;
//                         }
//                         Ordering::Greater => l = i + 1,
//                     }
//                 }
//             }
//             Inner::Tree(_) => todo!(),
//         }
//     }
// }
