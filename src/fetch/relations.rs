use core::{
    fmt::{self, Formatter},
    slice,
};

use alloc::vec::Vec;
use atomic_refcell::AtomicRef;
use itertools::Itertools;
use smallvec::SmallVec;

use crate::{
    archetype::{Archetype, Slot},
    dummy, Access, AccessKind, Component, ComponentValue, Entity, Fetch, FetchItem, RelationExt,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch};

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
                .relations_like(self.component.id())
                .map(|(info, cell)| {
                    (
                        info.object.unwrap(),
                        AtomicRef::map(cell.storage().borrow(), |v| unsafe { v.borrow() }),
                    )
                })
                .collect()
        };

        Some(PreparedRelations { borrows })
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

/// Query all relations of the specified kind
pub fn relations_like<T: ComponentValue>(relation: impl RelationExt<T>) -> Relations<T> {
    Relations {
        component: relation.of(dummy()),
    }
}
