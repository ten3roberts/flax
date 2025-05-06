use core::fmt::Formatter;

use crate::{
    archetype::{Archetype, CellMutGuard, ChangeKind, Slice},
    component::ComponentValue,
    fetch::{FetchAccessData, FetchPrepareData, PreparedFetch},
    system::Access,
    util::PtrMut,
    Component, Fetch, FetchItem,
};

use alloc::vec::Vec;

use super::change::ChangeCursor;

#[derive(Clone)]
/// Filter which only yields for change events
pub struct ChangeFilterMut<T> {
    component: Component<T>,
    kind: ChangeKind,
}

impl<T: ComponentValue> core::fmt::Debug for ChangeFilterMut<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ModifiedFilter")
            .field("component", &self.component)
            .field("kind", &self.kind)
            .finish()
    }
}

impl<T: ComponentValue> ChangeFilterMut<T> {
    /// Create a new modified filter
    pub(crate) fn new(component: Component<T>, kind: ChangeKind) -> Self {
        Self { component, kind }
    }
}

impl<'q, T> FetchItem<'q> for ChangeFilterMut<T>
where
    T: ComponentValue,
{
    type Item = &'q mut T;
}

impl<'w, T> Fetch<'w> for ChangeFilterMut<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedChangeFilterMut<'w, T>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let cell = data.arch.cell(self.component.key())?;
        let guard = cell.borrow_mut();

        // Make sure to enable modification tracking if it is actively used
        if self.kind.is_modified() {
            guard.changes().set_track_modified()
        }

        Some(PreparedChangeFilterMut {
            data: guard,
            kind: self.kind,
            arch: data.arch,
            tick: data.new_tick,
            cursor: ChangeCursor::new(data.old_tick),
        })
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.component.filter_arch(data)
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.component.access(data, dst);
    }

    fn describe(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{} {}", self.kind, self.component.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        searcher.add_required(self.component.key())
    }
}

#[doc(hidden)]
pub struct PreparedChangeFilterMut<'w, T> {
    data: CellMutGuard<'w, [T]>,
    kind: ChangeKind,
    cursor: ChangeCursor,
    arch: &'w Archetype,
    tick: u32,
}

impl<T> core::fmt::Debug for PreparedChangeFilterMut<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PreparedChangeFilterMut")
            .finish_non_exhaustive()
    }
}

impl<'q, T: ComponentValue> PreparedFetch<'q> for PreparedChangeFilterMut<'_, T> {
    type Item = &'q mut T;
    type Chunk = PtrMut<'q, T>;

    const HAS_FILTER: bool = true;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.data
            .set_modified(&self.arch.entities[slots.as_range()], slots, self.tick);

        PtrMut::new(self.data.get_mut()[slots.as_range()].as_mut_ptr())
    }

    #[inline]
    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let old = chunk.as_ptr();
        chunk.advance(1);
        &mut *old
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let cur = match self
            .cursor
            .find_slice(self.data.changes().get(self.kind).as_slice(), slots)
        {
            Some(v) => v,
            None => return Slice::new(slots.end, slots.end),
        };

        cur.intersect(&slots)
            .unwrap_or(Slice::new(slots.end, slots.end))
    }
}
