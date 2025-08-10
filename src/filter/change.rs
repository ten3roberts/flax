use alloc::vec::Vec;
use core::fmt::Formatter;
use itertools::Itertools;

use crate::archetype::{CellGuard, Change, Slot};
use crate::component::ComponentValue;
use crate::fetch::{FetchAccessData, FetchPrepareData, PreparedFetch, RandomFetch};
use crate::system::Access;
use crate::util::Ptr;
use crate::{
    archetype::{ChangeKind, Slice},
    Component, Fetch, FetchItem,
};

#[derive(Clone)]
/// Filter which only yields for change events
pub struct ChangeFilter<T> {
    component: Component<T>,
    kind: ChangeKind,
}

impl<T: ComponentValue> core::fmt::Debug for ChangeFilter<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ModifiedFilter")
            .field("component", &self.component)
            .field("kind", &self.kind)
            .finish()
    }
}

impl<T: ComponentValue> ChangeFilter<T> {
    /// Create a new modified filter
    pub(crate) fn new(component: Component<T>, kind: ChangeKind) -> Self {
        Self { component, kind }
    }
}

impl<'q, T> FetchItem<'q> for ChangeFilter<T>
where
    T: ComponentValue,
{
    type Item = &'q T;
}

impl<'q, T: ComponentValue> RandomFetch<'q> for PreparedChangeFilter<'_, T> {
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
        match &self.data {
            ChangeFilterData::Component(guard) => {
                unsafe { guard.get().get_unchecked(slot) }
            }
            ChangeFilterData::ExternalRemoval(_) => {
                // For external removals, we can't return actual component data
                // This should never be called in practice for removal filters
                // since they only use the filtering logic
                panic!("Cannot fetch component data for removed components")
            }
        }
    }

    #[inline]
    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
        chunk.add(slot).as_ref()
    }
}

impl<'w, T> Fetch<'w> for ChangeFilter<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedChangeFilter<'w, T>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let cell = data.arch.cell(self.component.key());
        
        let filter_data = if let Some(cell) = cell {
            // Component exists in archetype - use normal cell data
            let guard = cell.borrow();

            // Make sure to enable modification tracking if it is actively used
            if self.kind.is_modified() {
                guard.changes().set_track_modified()
            }

            ChangeFilterData::Component(guard)
        } else if matches!(self.kind, ChangeKind::Removed) {
            // For removal filters, check if we have external removal data
            if let Some(external_data) = data.arch.get_external_removal(self.component.key()) {
                ChangeFilterData::ExternalRemoval(external_data)
            } else {
                // No external removal data available
                return None;
            }
        } else {
            // Component doesn't exist and it's not a removal filter
            return None;
        };

        Some(PreparedChangeFilter {
            data: filter_data,
            kind: self.kind,
            cursor: ChangeCursor::new(data.old_tick),
        })
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        if matches!(self.kind, ChangeKind::Removed) {
            // For removal filters, we want to consider all archetypes since:
            // 1. Archetypes with the component may have normal removal tracking
            // 2. Archetypes without the component may have external removal data
            true
        } else {
            // For added/modified filters, the component must exist
            self.component.filter_arch(data)
        }
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.component.access(data, dst);
    }

    fn describe(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{} {}", self.kind, self.component.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        if matches!(self.kind, ChangeKind::Removed) {
            // For removal filters, we don't add any archetype requirements
            // since we need to check all archetypes for external removal data
            // regardless of their component structure
            
            // Note: The query system's archetype caching might miss new entity locations
            // after removals, so removal filters may need special handling to ensure
            // all archetypes are re-checked when entities move between archetypes
        } else {
            // For added/modified filters, the component must be present
            searcher.add_required(self.component.key());
        }
    }
}

struct ChangeCursor {
    cursor: usize,
    old_tick: u32,
    cur: Option<Slice>,
}

impl ChangeCursor {
    fn new(old_tick: u32) -> Self {
        Self {
            cursor: 0,
            old_tick,
            cur: None,
        }
    }

    pub(crate) fn find_slice(&mut self, changes: &[Change], slots: Slice) -> Option<Slice> {
        // Short circuit
        if let Some(cur) = self.cur {
            if cur.overlaps(slots) {
                return Some(cur);
            }
        }

        let change = changes[self.cursor..]
            .iter()
            .filter(|v| v.tick > self.old_tick)
            .find_position(|change| change.slice.overlaps(slots));

        if let Some((idx, change)) = change {
            self.cur = Some(change.slice);
            self.cursor = idx;
            return Some(change.slice);
        }

        let change = changes[..self.cursor]
            .iter()
            .filter(|v| v.tick > self.old_tick)
            .find_position(|change| change.slice.overlaps(slots));

        if let Some((_, change)) = change {
            return Some(change.slice);
        }

        None
    }
}

/// Data source for change filtering
enum ChangeFilterData<'w, T> {
    /// Component exists in archetype - use normal cell data
    Component(CellGuard<'w, [T]>),
    /// Component doesn't exist - use external removal tracking
    ExternalRemoval(&'w crate::archetype::ExternalRemovalData),
}

#[doc(hidden)]
pub struct PreparedChangeFilter<'w, T> {
    data: ChangeFilterData<'w, T>,
    kind: ChangeKind,
    cursor: ChangeCursor,
}

impl<T> core::fmt::Debug for PreparedChangeFilter<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PreparedChangeFilter")
            .finish_non_exhaustive()
    }
}

impl<'q, T: ComponentValue> PreparedFetch<'q> for PreparedChangeFilter<'_, T> {
    type Item = &'q T;
    type Chunk = Ptr<'q, T>;

    const HAS_FILTER: bool = true;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        match &self.data {
            ChangeFilterData::Component(guard) => {
                Ptr::new(guard.get()[slots.as_range()].as_ptr())
            }
            ChangeFilterData::ExternalRemoval(_) => {
                // For external removals, we don't have actual component data
                // We just need a dummy pointer for the filter logic
                // This is safe because removed filters only use the change tracking data,
                // not the actual component values
                Ptr::new(core::ptr::null::<T>())
            }
        }
    }

    #[inline]
    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let old = chunk.as_ptr();
        chunk.advance(1);
        &*old
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let changes = match &self.data {
            ChangeFilterData::Component(guard) => {
                guard.changes().get(self.kind).as_slice()
            }
            ChangeFilterData::ExternalRemoval(external_data) => {
                external_data.changes().get(self.kind).as_slice()
            }
        };

        let cur = match self.cursor.find_slice(changes, slots) {
            Some(v) => v,
            None => return Slice::new(slots.end, slots.end),
        };

        cur.intersect(&slots)
            .unwrap_or(Slice::new(slots.end, slots.end))
    }
}

#[doc(hidden)]
#[cfg(test)]
pub struct ChangeFetch<'w> {
    changes: &'w [Change],
    cursor: ChangeCursor,
}

#[cfg(test)]
impl<'w> ChangeFetch<'w> {
    pub fn new(changes: &'w [Change], new_tick: u32) -> Self {
        Self {
            changes,
            cursor: ChangeCursor::new(new_tick),
        }
    }
}

#[cfg(test)]
impl<'q> RandomFetch<'q> for ChangeFetch<'_> {
    #[inline]
    unsafe fn fetch_shared(&'q self, _: Slot) -> Self::Item {}

    #[inline]
    unsafe fn fetch_shared_chunk(_: &Self::Chunk, _: Slot) -> Self::Item {}
}

#[cfg(test)]
impl<'q> PreparedFetch<'q> for ChangeFetch<'_> {
    type Item = ();
    type Chunk = ();
    const HAS_FILTER: bool = true;

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let cur = match self.cursor.find_slice(self.changes, slots) {
            Some(v) => v,
            None => return Slice::new(slots.end, slots.end),
        };

        cur.intersect(&slots)
            .unwrap_or(Slice::new(slots.end, slots.end))
    }

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {}

    #[inline]
    unsafe fn fetch_next(_: &mut Self::Chunk) -> Self::Item {}
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use crate::{archetype::Change, filter::FilterIter};

    use super::*;

    #[test]
    fn filter_slices() {
        let changes = [
            Change::new(Slice::new(10, 20), 3),
            Change::new(Slice::new(20, 22), 4),
            Change::new(Slice::new(30, 80), 3),
            Change::new(Slice::new(100, 200), 4),
        ];

        let mut filter = ChangeFetch {
            changes: &changes[..],
            cursor: ChangeCursor::new(2),
        };

        unsafe {
            assert_eq!(filter.filter_slots(Slice::new(0, 10)), Slice::new(10, 10));
            assert_eq!(filter.filter_slots(Slice::new(0, 50)), Slice::new(10, 20));
            assert_eq!(filter.filter_slots(Slice::new(20, 50)), Slice::new(20, 22));
            assert_eq!(filter.filter_slots(Slice::new(22, 50)), Slice::new(30, 50));

            assert_eq!(filter.filter_slots(Slice::new(0, 10)), Slice::new(10, 10));
            // Due to modified state
            assert_eq!(filter.filter_slots(Slice::new(0, 50)), Slice::new(30, 50));

            assert_eq!(
                filter.filter_slots(Slice::new(120, 500)),
                Slice::new(120, 200)
            );
        }
    }

    #[test]
    fn filter_slices_consume() {
        let changes = [
            Change::new(Slice::new(10, 20), 3),
            Change::new(Slice::new(20, 22), 4),
            Change::new(Slice::new(30, 80), 3),
            Change::new(Slice::new(100, 200), 4),
        ];

        let filter = ChangeFetch {
            changes: &changes[..],
            cursor: ChangeCursor::new(2),
        };

        let slices = FilterIter::new(Slice::new(0, 500), filter).collect_vec();

        assert_eq!(
            &[
                Slice::new(10, 20),
                Slice::new(20, 22),
                Slice::new(30, 80),
                Slice::new(100, 200),
            ],
            &slices[..]
        );
    }

    #[test]
    fn filter_slices_partial() {
        let changes = [
            Change::new(Slice::new(10, 20), 3),
            Change::new(Slice::new(20, 22), 4),
            Change::new(Slice::new(30, 80), 3),
            Change::new(Slice::new(100, 200), 4),
        ];

        let filter = ChangeFetch {
            changes: &changes[..],
            cursor: ChangeCursor::new(2),
        };

        let slices = FilterIter::new(Slice::new(25, 150), filter)
            .take(100)
            .collect_vec();

        assert_eq!(&[Slice::new(30, 80), Slice::new(100, 150),], &slices[..]);
    }
}
