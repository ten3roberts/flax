use core::slice;

use atomic_refcell::{AtomicRef, AtomicRefMut};
use itertools::Itertools;
use smallvec::SmallVec;

use crate::{
    archetype::{Archetype, Changes, Slice, Slot},
    wildcard, AccessKind, ArchetypeId, Change, Component, ComponentValue,
};

use super::*;

#[doc(hidden)]
pub struct PreparedComponentMut<'a, T> {
    borrow: AtomicRefMut<'a, [T]>,
    changes: AtomicRefMut<'a, Changes>,
}

#[doc(hidden)]
pub struct PreparedComponent<'a, T> {
    borrow: AtomicRef<'a, [T]>,
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for PreparedComponent<'w, T> {
    type Item = &'q T;

    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        &self.borrow[slot]
    }
}

impl<'w, T> Fetch<'w> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedComponent<'w, T>;

    fn prepare(&self, _: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.borrow(*self)?;
        Some(PreparedComponent { borrow })
    }

    fn matches(&self, _: &'w World, archetype: &'w Archetype) -> bool {
        archetype.has(self.id())
    }

    fn describe(&self) -> String {
        self.name().to_string()
    }

    fn difference(&self, archetype: &Archetype) -> Vec<String> {
        if archetype.has(self.id()) {
            vec![]
        } else {
            vec![self.name().to_string()]
        }
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        if archetype.has(self.id()) {
            vec![Access {
                kind: AccessKind::Archetype {
                    id,
                    component: self.id(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }
}

#[derive(Debug, Clone)]
/// Mutable component fetch
/// See [crate::Component::as_mut]
pub struct Mutable<T: ComponentValue>(pub(crate) Component<T>);

impl<'w, T> Fetch<'w> for Mutable<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = true;

    type Prepared = PreparedComponentMut<'w, T>;

    fn prepare(&self, _: &'w World, archetype: &'w Archetype) -> Option<Self::Prepared> {
        let borrow = archetype.borrow_mut(self.0)?;
        let changes = archetype.changes_mut(self.0.id())?;

        Some(PreparedComponentMut { borrow, changes })
    }

    fn matches(&self, _: &'w World, archetype: &'w Archetype) -> bool {
        archetype.has(self.0.id())
    }
    fn describe(&self) -> String {
        format!("mut {}", self.0.name())
    }

    fn difference(&self, archetype: &Archetype) -> Vec<String> {
        if archetype.has(self.0.id()) {
            vec![]
        } else {
            vec![self.0.name().to_string()]
        }
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        if archetype.has(self.0.id()) {
            vec![Access {
                kind: AccessKind::Archetype {
                    id,
                    component: self.0.id(),
                },
                mutable: true,
            }]
        } else {
            vec![]
        }
    }
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for PreparedComponentMut<'w, T> {
    type Item = &'q mut T;

    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        (&mut self.borrow[slot] as *mut T)
            .as_mut()
            .expect("Non null")
    }

    unsafe fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        self.changes.set(Change::modified(slots, change_tick));
    }
}

/// Query all relations of the specified kind
pub fn relations_like<T: ComponentValue>(relation: fn(Entity) -> Component<T>) -> Relations<T> {
    Relations {
        component: relation(wildcard()),
    }
}

/// Returns a list of relations with the specified kind
#[derive(Debug, Clone)]
pub struct Relations<T: ComponentValue> {
    component: Component<T>,
}

impl<'a, T> Fetch<'a> for Relations<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedRelations<'a, T>;

    fn prepare(&self, world: &'a World, arch: &'a Archetype) -> Option<Self::Prepared> {
        let relation = self.component.id().low();
        let borrows: SmallVec<[(Entity, AtomicRef<[T]>); 4]> = {
            arch.storage()
                .iter()
                .map(move |(k, v)| {
                    let (rel, obj) = k.split_pair();
                    (rel, obj, k, v)
                })
                .filter(move |(rel, _, k, _)| k.is_relation() && *rel == relation)
                // Safety:
                // Since the component is the same except for the object,
                // the component type is guaranteed to be the same
                .map(|(_, obj, _, v)| {
                    (
                        world
                            .reconstruct(obj)
                            .expect("Relation object is not alive"),
                        unsafe { v.borrow::<T>() },
                    )
                })
                .collect()
        };

        Some(PreparedRelations { borrows })
    }

    fn matches(&self, _: &'a World, _: &'a Archetype) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("relations({})", self.component.name())
    }

    fn difference(&self, _: &Archetype) -> Vec<String> {
        vec![]
    }

    fn access(&self, id: ArchetypeId, arch: &Archetype) -> Vec<Access> {
        let relation = self.component.id().low();
        arch.storage()
            .keys()
            .filter(move |k| k.is_relation() && k.low() == relation)
            .map(|&k| Access {
                kind: AccessKind::Archetype { id, component: k },
                mutable: false,
            })
            .collect_vec()
    }
}

#[doc(hidden)]
pub struct PreparedRelations<'a, T> {
    borrows: SmallVec<[(Entity, AtomicRef<'a, [T]>); 4]>,
}

impl<'q, 'w, T> PreparedFetch<'q> for PreparedRelations<'w, T>
where
    T: ComponentValue,
{
    type Item = RelationsIter<'q, T>;

    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        RelationsIter {
            borrows: self.borrows.iter(),
            slot,
        }
    }
}

/// Iterates the relation object and data for the yielded query item
pub struct RelationsIter<'a, T> {
    borrows: slice::Iter<'a, (Entity, AtomicRef<'a, [T]>)>,
    slot: Slot,
}

impl<'a, T> Iterator for RelationsIter<'a, T> {
    type Item = (Entity, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let (id, borrow) = self.borrows.next()?;
        let borrow = &borrow[self.slot];
        Some((*id, borrow))
    }
}
