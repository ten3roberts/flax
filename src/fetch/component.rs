use core::slice;

use atomic_refcell::{AtomicRef, AtomicRefMut};

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
        let borrow = archetype.storage(*self)?;
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
            eprintln!("Archetype has: {:?}", self.name());
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
        let borrow = archetype.storage_mut(self.0)?;
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
            eprintln!("Archetype has mut: {:?}", self.0.name());

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

/// Similar to a component fetch, with the difference that it also yields the
/// object entity.
#[derive(Debug, Clone)]
pub struct Relation<T: ComponentValue> {
    component: Component<T>,
    index: usize,
}

impl<T: ComponentValue> Relation<T> {
    /// Creates a new relation fetch
    pub fn new(component: Component<T>, index: usize) -> Self {
        Self { component, index }
    }
}

impl<'a, T> Fetch<'a> for Relation<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedPair<'a, T>;

    fn prepare(&self, world: &'a World, archetype: &'a Archetype) -> Option<Self::Prepared> {
        let (sub, obj) = self.component.id().split_pair();
        if obj == wildcard().low() {
            let (obj, borrow) = archetype
                .components()
                .filter(|v| v.id().low() == sub)
                .skip(self.index)
                .map(|v| {
                    let (sub1, obj) = v.id().split_pair();
                    assert_eq!(sub1, sub);
                    let borrow = archetype.storage::<T, _>(v.id()).unwrap();
                    let obj = world.reconstruct(obj).unwrap();
                    (obj, borrow)
                })
                .next()?;

            Some(PreparedPair { borrow, obj })
        } else {
            todo!()
        }
    }

    fn matches(&self, _: &'a World, archetype: &'a Archetype) -> bool {
        let (sub, obj) = self.component.id().split_pair();
        if obj == wildcard().low() {
            archetype
                .components()
                .filter(|component| component.id().low() == sub)
                .nth(self.index)
                .is_some()
        } else {
            archetype.has(self.component.id())
        }
    }

    fn describe(&self) -> String {
        format!("relation({})[{}]", self.component.name(), self.index)
    }

    fn difference(&self, archetype: &Archetype) -> Vec<String> {
        let (sub, obj) = self.component.id().split_pair();
        if obj == wildcard().low() {
            if archetype
                .components()
                .filter(|component| component.id().low() == sub)
                .nth(self.index)
                .is_some()
            {
                vec![]
            } else {
                vec![self.component.name().to_string()]
            }
        } else if archetype.has(self.component.id()) {
            vec![]
        } else {
            vec![self.component.name().to_string()]
        }
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        let (sub, obj) = self.component.id().split_pair();
        if obj == wildcard().low() {
            let borrow = archetype
                .components()
                .filter(|v| v.id().low() == sub)
                .skip(self.index)
                .map(|v| Access {
                    kind: AccessKind::Archetype {
                        id,
                        component: v.id(),
                    },
                    mutable: false,
                })
                .next();

            if let Some(borrow) = borrow {
                vec![borrow]
            } else {
                vec![]
            }
        } else {
            todo!()
        }
    }
}

#[doc(hidden)]
pub struct PreparedPair<'a, T> {
    borrow: AtomicRef<'a, [T]>,
    obj: Entity,
}

impl<'q, 'w, T> PreparedFetch<'q> for PreparedPair<'w, T>
where
    T: ComponentValue,
{
    type Item = (Entity, &'q T);

    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        let item = &self.borrow[slot];
        (self.obj, item)
    }
}
