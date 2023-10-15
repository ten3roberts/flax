use core::{
    fmt::{self, Formatter},
    slice,
};

use alloc::vec::Vec;
use smallvec::SmallVec;

use crate::{
    archetype::{CellGuard, Slot},
    component::{dummy, ComponentValue},
    system::{Access, AccessKind},
    Component, Entity, Fetch, FetchItem, RelationExt,
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
        let borrows: SmallVec<[_; 4]> = {
            data.arch
                .relations_like(self.component.id())
                .map(|(desc, cell)| (desc.object.unwrap(), cell.borrow()))
                .collect()
        };

        Some(PreparedRelations { borrows })
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        let relation = self.component.key().id;
        dst.extend(data.arch.cells().keys().filter_map(move |k| {
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
        }))
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
    borrows: SmallVec<[(Entity, CellGuard<'a, [T]>); 4]>,
}

pub struct Batch<'a, T> {
    borrows: &'a [(Entity, CellGuard<'a, [T]>)],
    slot: Slot,
}

impl<'w, 'q, T> PreparedFetch<'q> for PreparedRelations<'w, T>
where
    T: ComponentValue,
{
    type Item = RelationsIter<'q, T>;

    type Chunk = Batch<'q, T>;

    unsafe fn create_chunk(&'q mut self, slice: crate::archetype::Slice) -> Self::Chunk {
        Batch {
            borrows: &self.borrows,
            slot: slice.start,
        }
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let slot = chunk.slot;
        chunk.slot += 1;

        RelationsIter {
            borrows: chunk.borrows.iter(),
            slot,
        }
    }
}

/// Iterates the relation object and data for the yielded query item
pub struct RelationsIter<'a, T> {
    borrows: slice::Iter<'a, (Entity, CellGuard<'a, [T]>)>,
    slot: Slot,
}

impl<'a, T> Iterator for RelationsIter<'a, T> {
    type Item = (Entity, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let (id, borrow) = self.borrows.next()?;
        let borrow = &borrow.get()[self.slot];
        Some((*id, borrow))
    }
}

/// Query all relations of the specified kind
pub fn relations_like<T: ComponentValue>(relation: impl RelationExt<T>) -> Relations<T> {
    Relations {
        component: relation.of(dummy()),
    }
}
