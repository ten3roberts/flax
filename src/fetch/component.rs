use core::slice;

use atomic_refcell::{AtomicRef, AtomicRefMut};
use itertools::Itertools;
use smallvec::SmallVec;

use crate::{
    archetype::{Changes, Slice, Slot},
    dummy, AccessKind, Change, Component, ComponentValue, RelationExt,
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

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Safety: bounds guaranteed by callee
        self.borrow.get_unchecked(slot)
    }
}

impl<'w, T> Fetch<'w> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;
    type Filter = Nothing;

    type Prepared = PreparedComponent<'w, T>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrow = data.arch.borrow(self.key())?;
        Some(PreparedComponent { borrow })
    }

    fn matches(&self, data: FetchPrepareData) -> bool {
        data.arch.has(self.key())
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        if data.arch.has(self.key()) {
            vec![Access {
                kind: AccessKind::Archetype {
                    id: data.arch_id,
                    component: self.key(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }

    fn describe(&self, f: &mut dyn Write) -> fmt::Result {
        f.write_str(self.name())
    }

    fn filter(&self) -> Self::Filter {
        Nothing
    }

    fn components(&self, result: &mut Vec<ComponentKey>) {
        result.push(self.key())
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for Component<T> {
    type Item = &'q T;
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
    const HAS_FILTER: bool = false;

    type Filter = Nothing;

    type Prepared = PreparedComponentMut<'w, T>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrow = data.arch.borrow_mut(self.0)?;
        let changes = data.arch.changes_mut(self.0.key())?;

        Some(PreparedComponentMut { borrow, changes })
    }

    fn matches(&self, data: FetchPrepareData) -> bool {
        data.arch.has(self.0.key())
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        if data.arch.has(self.0.key()) {
            vec![
                Access {
                    kind: AccessKind::Archetype {
                        id: data.arch_id,
                        component: self.0.key(),
                    },
                    mutable: true,
                },
                Access {
                    kind: AccessKind::ChangeEvent {
                        id: data.arch_id,
                        component: self.0.key(),
                    },
                    mutable: true,
                },
            ]
        } else {
            vec![]
        }
    }

    fn describe(&self, f: &mut dyn Write) -> fmt::Result {
        f.write_str("mut ")?;
        f.write_str(self.0.name())
    }
    fn filter(&self) -> Self::Filter {
        Nothing
    }

    fn components(&self, result: &mut Vec<ComponentKey>) {
        result.push(self.0.key())
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for Mutable<T> {
    type Item = &'q mut T;
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for PreparedComponentMut<'w, T> {
    type Item = &'q mut T;

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        &mut *(self.borrow.get_unchecked_mut(slot) as *mut T)
    }

    unsafe fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        self.changes
            .set_modified_if_tracking(Change::new(slots, change_tick));
    }
}

/// Query all relations of the specified kind
pub fn relations_like<T: ComponentValue>(relation: fn(Entity) -> Component<T>) -> Relations<T> {
    Relations {
        component: relation.of(dummy()),
    }
}

/// Returns a list of relations with the specified kind
#[derive(Debug, Clone)]
pub struct Relations<T: ComponentValue> {
    component: Component<T>,
}

impl<'w, T> Fetch<'w> for Relations<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;
    type Filter = Nothing;

    type Prepared = PreparedRelations<'w, T>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrows: SmallVec<[(Entity, AtomicRef<[T]>); 4]> = {
            data.arch
                .storage()
                .iter()
                .filter_map(move |(k, v)| {
                    if let Some(object) = k.object {
                        if k.id == self.component.key().id {
                            return Some((
                                object,
                                // Safety:
                                // Since the component is the same except for the object,
                                // the component type is guaranteed to be the same
                                unsafe { v.borrow::<T>() },
                            ));
                        }
                    }

                    None
                })
                .collect()
        };

        Some(PreparedRelations { borrows })
    }

    fn matches(&self, _: FetchPrepareData) -> bool {
        true
    }

    fn describe(&self, f: &mut dyn Write) -> fmt::Result {
        write!(f, "relations({})", self.component.name())
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        let relation = self.component.key().id;
        data.arch
            .storage()
            .keys()
            .filter_map(move |k| {
                if let Some(object) = k.object {
                    if k.id == self.component.key().id {
                        return Some(Access {
                            kind: AccessKind::Archetype {
                                id: data.arch_id,
                                component: *k,
                            },
                            mutable: false,
                        });
                    }
                }

                None
            })
            .collect_vec()
    }

    fn filter(&self) -> Self::Filter {
        Nothing
    }

    fn components(&self, _: &mut Vec<ComponentKey>) {}
}

impl<'q, T: ComponentValue> FetchItem<'q> for Relations<T> {
    type Item = RelationsIter<'q, T>;
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
