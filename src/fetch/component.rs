use core::slice;

use atomic_refcell::AtomicRef;
use itertools::Itertools;
use smallvec::SmallVec;

use crate::{archetype::Slot, dummy, AccessKind, Component, ComponentValue, RelationExt};

use super::{peek::PeekableFetch, *};

#[doc(hidden)]
pub struct ReadComponent<'a, T> {
    borrow: AtomicRef<'a, [T]>,
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for ReadComponent<'w, T> {
    type Item = &'q T;

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Safety: bounds guaranteed by callee
        unsafe { self.borrow.get_unchecked(slot) }
    }
}

impl<'w, 'p, T: ComponentValue> PeekableFetch<'p> for ReadComponent<'w, T> {
    type Peek = &'p T;

    unsafe fn peek(&'p self, slot: Slot) -> Self::Peek {
        self.borrow.get_unchecked(slot)
    }
}

impl<'w, T> Fetch<'w> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = ReadComponent<'w, T>;

    #[inline]
    fn prepare(&self, data: FetchPrepareData<'w>) -> Self::Prepared {
        let borrow = data.arch.borrow(self.key()).unwrap();
        ReadComponent { borrow }
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.key())
    }

    #[inline]
    fn access(&self, data: FetchAccessData) -> Vec<Access> {
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

    fn prepare(&self, data: FetchPrepareData<'w>) -> Self::Prepared {
        let borrows: SmallVec<[(Entity, AtomicRef<[T]>); 4]> = {
            data.arch
                .relations_like(self.component.id())
                .map(|(info, cell)| {
                    (
                        info.object.unwrap(),
                        AtomicRef::map(cell.storage().borrow(), |v| unsafe { v.borrow() }),
                    )
                })
                .collect()
        };

        PreparedRelations { borrows }
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData) -> Vec<Access> {
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
