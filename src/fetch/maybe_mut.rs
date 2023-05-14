use alloc::vec;
use alloc::vec::Vec;

use core::marker::PhantomData;

use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Cell, RefMut, Slot},
    system::{Access, AccessKind},
    Component, ComponentValue, Entity, Fetch, FetchItem,
};

use super::{PreparedFetch, ReadOnlyFetch};

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

    fn filter_arch(&self, arch: &crate::archetype::Archetype) -> bool {
        arch.has(self.0.key())
    }

    fn access(&self, data: super::FetchAccessData) -> Vec<Access> {
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

impl<'w, 'q, T: ComponentValue> PreparedFetch<'q> for PreparedMaybeMut<'w, T> {
    type Item = MutGuard<'q, T>;

    #[inline]
    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        MutGuard {
            slot,
            cell: self.cell,
            new_tick: self.new_tick,
            entities: self.entities,
            _marker: PhantomData,
        }
    }
}

impl<'w, 'q, T: ComponentValue> ReadOnlyFetch<'q> for PreparedMaybeMut<'w, T> {
    #[inline]
    unsafe fn fetch_shared(&'q self, slot: usize) -> Self::Item {
        MutGuard {
            slot,
            cell: self.cell,
            new_tick: self.new_tick,
            entities: self.entities,
            _marker: PhantomData,
        }
    }
}

/// Protects against accidental mutation.
///
/// See: [`MaybeMut`]
pub struct MutGuard<'w, T> {
    slot: Slot,
    entities: &'w [Entity],
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
        unsafe {
            self.cell
                .get_mut(self.entities, self.slot, self.new_tick)
                .unwrap()
        }
    }
}
