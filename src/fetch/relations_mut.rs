use core::{
    fmt::{self, Formatter},
    slice,
};

use alloc::vec::Vec;
use smallvec::SmallVec;

use crate::{
    archetype::{Archetype, CellMutGuard, Slice, Slot},
    component::ComponentValue,
    relation::Relation,
    system::{Access, AccessKind},
    util::PtrMut,
    Entity, Fetch, FetchItem, RelationExt,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch};

/// Returns an iterator of all relations of the specified type
#[derive(Debug, Clone)]
pub struct RelationsMut<T: ComponentValue> {
    relation: Relation<T>,
}

impl<'w, T> Fetch<'w> for RelationsMut<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedRelationsMut<'w, T>;

    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrows: SmallVec<[_; 4]> = {
            data.arch
                .relations_like(self.relation.id())
                .map(|(desc, &cell_index)| {
                    (
                        desc.target.unwrap(),
                        data.arch.cells()[cell_index].borrow_mut(),
                    )
                })
                .collect()
        };

        Some(PreparedRelationsMut {
            borrows,
            arch: data.arch,
            tick: data.new_tick,
        })
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
            mutable: true,
        });

        dst.extend(val);
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "relations({})", self.relation)
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for RelationsMut<T> {
    type Item = RelationsIterMut<'q, T>;
}

#[doc(hidden)]
pub struct PreparedRelationsMut<'a, T> {
    borrows: SmallVec<[(Entity, CellMutGuard<'a, [T]>); 4]>,
    arch: &'a Archetype,
    tick: u32,
}

pub struct Batch<'a, T> {
    borrows: PtrMut<'a, (Entity, CellMutGuard<'a, [T]>)>,
    borrow_count: usize,
    slot: Slot,
}

impl<'q, T> PreparedFetch<'q> for PreparedRelationsMut<'_, T>
where
    T: 'q + ComponentValue,
{
    type Item = RelationsIterMut<'q, T>;

    type Chunk = Batch<'q, T>;

    const HAS_FILTER: bool = false;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        for (_target, borrow) in &mut self.borrows {
            borrow.set_modified(&self.arch.entities[slots.as_range()], slots, self.tick)
        }

        Batch {
            borrow_count: self.borrows.len(),
            borrows: PtrMut::new(self.borrows.as_mut_ptr() as _),
            slot: slots.start,
        }
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let slot = chunk.slot;
        chunk.slot += 1;

        RelationsIterMut {
            borrows: slice::from_raw_parts_mut(chunk.borrows.as_ptr() as _, chunk.borrow_count)
                .iter_mut(),
            slot,
        }
    }
}

/// Iterates the relation targets and data for the yielded query item
pub struct RelationsIterMut<'a, T> {
    borrows: slice::IterMut<'a, (Entity, CellMutGuard<'a, [T]>)>,
    slot: Slot,
}

impl<'a, T> Iterator for RelationsIterMut<'a, T> {
    type Item = (Entity, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        let (id, borrow) = self.borrows.next()?;
        let borrow = &mut borrow.get_mut()[self.slot];
        Some((*id, borrow))
    }
}

/// Access all relations of the specified type on the entity.
///
/// See: [`relations`](crate::fetch::relations::relations_like)
pub fn relations_like_mut<T: ComponentValue>(relation: impl RelationExt<T>) -> RelationsMut<T> {
    RelationsMut {
        relation: relation.as_relation(),
    }
}
