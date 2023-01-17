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
pub struct WriteComponent<'a, T> {
    borrow: AtomicRefMut<'a, [T]>,
    changes: AtomicRefMut<'a, Changes>,
}

#[doc(hidden)]
pub struct ReadComponent<'a, T> {
    borrow: AtomicRef<'a, [T]>,
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for ReadComponent<'w, T> {
    type Item = &'q T;

    #[inline(always)]
    fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Safety: bounds guaranteed by callee
        unsafe { self.borrow.get_unchecked(slot) }
    }

    #[inline]
    fn filter_slots(&mut self, slots: Slice) -> Slice {
        slots
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice, change_tick: u32) {}
}

impl<'w, T> Fetch<'w> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = ReadComponent<'w, T>;

    #[inline]
    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrow = data.arch.borrow(self.key())?;
        Some(ReadComponent { borrow })
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.key())
    }

    #[inline]
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

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        searcher.add_required(self.key())
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

    type Prepared = WriteComponent<'w, T>;

    #[inline]
    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let (borrow, changes) = data.arch.borrow_mut(self.0)?;

        Some(WriteComponent { borrow, changes })
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.0.key())
    }

    #[inline]
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

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("mut ")?;
        f.write_str(self.0.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        searcher.add_required(self.0.key())
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for Mutable<T> {
    type Item = &'q mut T;
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for WriteComponent<'w, T> {
    type Item = &'q mut T;

    #[inline(always)]
    fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        unsafe { &mut *(self.borrow.get_unchecked_mut(slot) as *mut T) }
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        self.changes
            .set_modified_if_tracking(Change::new(slots, change_tick));
    }

    #[inline]
    fn filter_slots(&mut self, slots: Slice) -> Slice {
        slots
    }
}

/// Query all relations of the specified kind
pub fn relations_like<T: ComponentValue>(relation: fn(Entity) -> Component<T>) -> Relations<T> {
    Relations {
        component: relation.of(dummy()),
    }
}

/// Returns a list of relations of a specified type
#[derive(Debug, Clone)]
pub struct Relations<T: ComponentValue> {
    component: Component<T>,
}

impl<'w, T> Fetch<'w> for Relations<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedRelations<'w, T>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrows: SmallVec<[(Entity, AtomicRef<[T]>); 4]> = {
            data.arch
                .cells()
                .iter()
                .filter_map(move |(k, v)| {
                    if let Some(object) = k.object {
                        if k.id == self.component.key().id {
                            return Some((
                                object,
                                // Safety:
                                // Since the component is the same except for the object,
                                // the component type is guaranteed to be the same
                                AtomicRef::map(v.storage().borrow(), |v| unsafe { v.borrow() }),
                            ));
                        }
                    }

                    None
                })
                .collect()
        };

        Some(PreparedRelations { borrows })
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        let relation = self.component.key().id;
        data.arch
            .cells()
            .keys()
            .filter_map(move |k| {
                if k.object.is_some() && k.id == relation {
                    return Some(Access {
                        kind: AccessKind::Archetype {
                            id: data.arch_id,
                            component: *k,
                        },
                        mutable: false,
                    });
                }

                None
            })
            .collect_vec()
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "relations({})", self.component.name())
    }

    fn searcher(&self, _: &mut crate::ArchetypeSearcher) {}
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

    fn fetch(&'q mut self, slot: Slot) -> Self::Item {
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
