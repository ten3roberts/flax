use alloc::vec::Vec;
use atomic_refcell::AtomicRef;
use core::marker::PhantomData;

use crate::{
    archetype::{Cell, RefMut, Slot},
    component::ComponentValue,
    system::{Access, AccessKind},
    Component, Entity, Fetch, FetchItem,
};

use super::{FetchAccessData, PreparedFetch, RandomFetch};

/// A query for conservative mutablility.
///
/// This is useful for not triggering change detection when a component in a query isn't always
/// modified.
///
/// Implements `ReadOnlyFetch` as the mutation is explicit and the returned reference is limited
/// to the loop body, rather than the iterator.
pub struct MaybeMut<T>(pub(crate) Component<T>);

impl<'q, T: ComponentValue> FetchItem<'q> for MaybeMut<T> {
    type Item = MutGuard<'q, T>;
}

impl<'w, T: ComponentValue> Fetch<'w> for MaybeMut<T> {
    const MUTABLE: bool = false;

    type Prepared = PreparedMaybeMut<'w, T>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let cell = data.arch.cell(self.0.key())?;
        Some(PreparedMaybeMut {
            cell,
            new_tick: data.new_tick,
            entities: data.arch.entities(),
            _marker: PhantomData,
        })
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        data.arch.has(self.0.key())
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        if data.arch.has(self.0.key()) {
            dst.extend_from_slice(&[Access {
                kind: AccessKind::Archetype {
                    id: data.arch_id,
                    component: self.0.key(),
                },
                mutable: true,
            }])
        }
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("mut ")?;
        f.write_str(self.0.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        searcher.add_required(self.0.key())
    }

    fn by_ref(&self) -> crate::filter::RefFetch<Self>
    where
        Self: Sized,
    {
        crate::filter::RefFetch(self)
    }
}

pub struct PreparedMaybeMut<'w, T> {
    cell: &'w Cell,
    new_tick: u32,
    entities: &'w [Entity],
    _marker: PhantomData<T>,
}

pub struct Batch<'a> {
    cell: &'a Cell,
    new_tick: u32,
    ids: &'a [Entity],
    slot: Slot,
}

impl<'w, 'q, T: ComponentValue> PreparedFetch<'q> for PreparedMaybeMut<'w, T> {
    type Item = MutGuard<'q, T>;
    type Chunk = Batch<'q>;

    const HAS_FILTER: bool = false;

    unsafe fn create_chunk(&'q mut self, slice: crate::archetype::Slice) -> Self::Chunk {
        Batch {
            cell: self.cell,
            new_tick: self.new_tick,
            ids: self.entities,
            slot: slice.start,
        }
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let slot = chunk.slot;
        chunk.slot += 1;

        MutGuard {
            slot,
            cell: chunk.cell,
            new_tick: chunk.new_tick,
            id: *chunk.ids.get_unchecked(slot),
            _marker: PhantomData,
        }
    }
}

impl<'w, 'q, T: ComponentValue> RandomFetch<'q> for PreparedMaybeMut<'w, T> {
    #[inline]
    unsafe fn fetch_shared(&'q self, slot: usize) -> Self::Item {
        MutGuard {
            slot,
            cell: self.cell,
            new_tick: self.new_tick,
            id: self.entities[slot],
            _marker: PhantomData,
        }
    }

    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
        MutGuard {
            slot,
            cell: chunk.cell,
            new_tick: chunk.new_tick,
            id: chunk.ids[slot],
            _marker: PhantomData,
        }
    }
}

/// Protects against accidental mutation.
///
/// See: [`MaybeMut`]
pub struct MutGuard<'w, T> {
    slot: Slot,
    id: Entity,
    cell: &'w Cell,
    new_tick: u32,
    _marker: PhantomData<T>,
}

impl<'w, T: ComponentValue> MutGuard<'w, T> {
    /// Acquire a shared reference to the current value without triggering a change
    pub fn read(&self) -> AtomicRef<T> {
        // Type is guaranteed by fetch constructor
        unsafe { self.cell.get(self.slot).unwrap() }
    }

    /// Acquire a mutable reference to the current value.
    ///
    /// Triggers a change
    pub fn write(&self) -> RefMut<T> {
        // Type is guaranteed by constructor
        self.cell
            .get_mut(self.id, self.slot, self.new_tick)
            .unwrap()
    }
}
