use core::{
    fmt::{self, Formatter},
    slice,
};

use alloc::vec::Vec;
use smallvec::SmallVec;

use crate::{
    archetype::{CellGuard, Slot},
    component::ComponentValue,
    relation::{Relation, RelationExt},
    system::{Access, AccessKind},
    Entity, Fetch, FetchItem,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch, RandomFetch};

/// Returns a list of relations of a specified type
#[derive(Debug, Clone)]
pub struct Relations<T: ComponentValue> {
    relation: Relation<T>,
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
                .relations_like(self.relation.id())
                .map(|(desc, cell)| (desc.target.unwrap(), cell.borrow()))
                .collect()
        };

        Some(PreparedRelations { borrows })
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        let relation = self.relation.id();
        let val = data.arch.relations_like(relation).map(|v| Access {
            kind: AccessKind::Archetype {
                id: data.arch_id,
                component: *v.0,
            },
            mutable: false,
        });

        dst.extend(val);
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "relations({})", self.relation)
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

    const HAS_FILTER: bool = false;

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

/// Iterates the relation targets and data for the yielded query item
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

/// Query all relations of the specified kind.
///
/// **Note**: This still matches if there are `0` relations.
pub fn relations_like<T: ComponentValue>(relation: impl RelationExt<T>) -> Relations<T> {
    Relations {
        relation: relation.as_relation(),
    }
}

/// Query the nth relation of the specified kind.
///
/// This is useful for [`Exclusive`](crate::metadata::Exclusive) relations where there is only one parent
///
/// **Note**: Fails to match if there is no nth relation, prefer using [`opt`](crate::FetchExt::opt) for
/// optional relations.
pub fn nth_relation<T: ComponentValue>(relation: impl RelationExt<T>, n: usize) -> NthRelation<T> {
    NthRelation {
        relation: relation.as_relation(),
        n,
    }
}

/// Returns the *nth* relation of a specified type
#[derive(Debug, Clone)]
pub struct NthRelation<T: ComponentValue> {
    relation: Relation<T>,
    n: usize,
}

impl<'w, T> Fetch<'w> for NthRelation<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedNthRelation<'w, T>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrow = data
            .arch
            .relations_like(self.relation.id)
            .nth(self.n)
            .map(|(desc, cell)| (desc.target.unwrap(), cell.borrow()))?;

        Some(PreparedNthRelation { borrow })
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        let relation = self.relation.id;
        let val = data
            .arch
            .relations_like(relation)
            .nth(self.n)
            .map(|v| Access {
                kind: AccessKind::Archetype {
                    id: data.arch_id,
                    component: *v.0,
                },
                mutable: false,
            });

        dst.extend(val);
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "relations({})", self.relation)
    }
}

impl<'q, T: ComponentValue> RandomFetch<'q> for PreparedNthRelation<'q, T> {
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
        let value = &self.borrow.1.get()[slot];
        (self.borrow.0, value)
    }

    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
        let (id, borrow) = &*chunk.borrow;

        (*id, &borrow.get()[slot])
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for NthRelation<T> {
    type Item = (Entity, &'q T);
}

#[doc(hidden)]
pub struct PreparedNthRelation<'a, T> {
    borrow: (Entity, CellGuard<'a, [T]>),
}

pub struct NthBatch<'a, T> {
    borrow: *const (Entity, CellGuard<'a, [T]>),
    slot: Slot,
}

impl<'w, 'q, T> PreparedFetch<'q> for PreparedNthRelation<'w, T>
where
    T: ComponentValue,
{
    type Item = (Entity, &'q T);

    type Chunk = NthBatch<'q, T>;

    const HAS_FILTER: bool = false;

    unsafe fn create_chunk(&'q mut self, slice: crate::archetype::Slice) -> Self::Chunk {
        NthBatch {
            borrow: &self.borrow,
            slot: slice.start,
        }
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let slot = chunk.slot;
        chunk.slot += 1;

        let (id, borrow) = unsafe { &*chunk.borrow };

        let borrow = &borrow.get()[slot];
        (*id, borrow)
    }
}
