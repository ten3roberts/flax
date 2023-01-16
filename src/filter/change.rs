use core::fmt::Formatter;
use core::ops::Deref;

use alloc::vec;
use alloc::vec::Vec;
use atomic_refcell::{AtomicRef, AtomicRefCell};

use crate::fetch::{FetchPrepareData, PreparedComponent, PreparedFetch};
use crate::{
    archetype::{ChangeList, Slice},
    Access, Archetype, ChangeKind, Component, ComponentValue, Fetch, FetchItem,
};

static EMPTY_CHANGELIST_CELL: AtomicRefCell<ChangeList> = AtomicRefCell::new(ChangeList::new());
static EMPTY_CHANGELIST: ChangeList = ChangeList::new();

#[derive(Clone)]
/// Filter which only yields modified or inserted components
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

impl<'w, T> Fetch<'w> for ChangeFilter<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedKindFilter<PreparedComponent<'w, T>, AtomicRef<'w, ChangeList>>;

    fn prepare(&'w self, data: crate::fetch::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let changes = data.arch.changes(self.component.key());

        let changes = if let Some(changes) = changes {
            // Make sure to enable modification tracking if it is actively used
            if self.kind.is_modified() {
                changes.set_track_modified()
            }

            AtomicRef::map(changes, |changes| changes.get(self.kind))
        } else {
            EMPTY_CHANGELIST_CELL.borrow()
        };

        let fetch = self.component.prepare(data)?;
        Some(PreparedKindFilter::new(fetch, changes, data.old_tick))
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        self.component.filter_arch(arch) && arch.changes(self.component.key()).is_some()
    }

    fn access(&self, data: crate::fetch::FetchPrepareData) -> Vec<Access> {
        let mut v = self.component.access(data);

        if self.filter_arch(data.arch) {
            v.push(Access {
                kind: crate::AccessKind::ChangeEvent {
                    id: data.arch_id,
                    component: self.component.key(),
                },
                mutable: false,
            })
        }

        v
    }

    fn describe(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{} {}", self.kind, self.component.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        searcher.add_required(self.component.key())
    }
}

#[derive(Debug)]
#[doc(hidden)]
pub struct PreparedKindFilter<Q, A> {
    fetch: Q,
    changes: A,
    cur: Option<Slice>,
    // The current change group.
    // Starts at the end and decrements
    cursor: usize,
    tick: u32,
}

impl<Q, A> PreparedKindFilter<Q, A>
where
    A: Deref<Target = ChangeList>,
{
    pub(crate) fn new(fetch: Q, changes: A, tick: u32) -> Self {
        Self {
            fetch,
            changes,
            cur: None,
            cursor: 0,
            tick,
        }
    }

    pub fn current_slice(&mut self) -> Option<Slice> {
        if let Some(cur) = self.cur {
            return Some(cur);
        }

        loop {
            let change = self.changes.get(self.cursor)?;
            self.cursor += 1;

            if change.tick > self.tick {
                return Some(*self.cur.insert(change.slice));
            }
        }
    }
}

impl<'q, Q, A> PreparedFetch<'q> for PreparedKindFilter<Q, A>
where
    Q: PreparedFetch<'q>,
    A: Deref<Target = ChangeList>,
{
    type Item = Q::Item;

    fn fetch(&'q mut self, slot: usize) -> Self::Item {
        self.fetch.fetch(slot)
    }

    fn filter_slots(&mut self, slots: Slice) -> Slice {
        loop {
            let cur = match self.current_slice() {
                Some(v) => v,
                None => return Slice::empty(),
            };

            let intersect = cur.intersect(&slots);
            // Try again with the next change group
            if intersect.is_empty() {
                self.cur = None;
                continue;
            } else {
                return intersect;
            }
        }
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        self.fetch.set_visited(slots, change_tick)
    }
}

#[derive(Clone)]
/// Filter which only yields removed `components
pub struct RemovedFilter<T> {
    component: Component<T>,
}

impl<T: ComponentValue> core::fmt::Debug for RemovedFilter<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RemovedFilter")
            .field("component", &self.component)
            .finish()
    }
}

impl<T: ComponentValue> RemovedFilter<T> {
    /// Create a new removed filter
    pub(crate) fn new(component: Component<T>) -> Self {
        Self { component }
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for RemovedFilter<T> {
    type Item = ();
}

impl<'a, T: ComponentValue> Fetch<'a> for RemovedFilter<T> {
    const MUTABLE: bool = false;

    type Prepared = PreparedKindFilter<(), &'a ChangeList>;

    fn prepare(&self, data: FetchPrepareData<'a>) -> Option<Self::Prepared> {
        let changes = data
            .arch
            .removals(self.component.key())
            .unwrap_or(&EMPTY_CHANGELIST);

        Some(PreparedKindFilter::new((), changes, data.old_tick))
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        if self.filter_arch(data.arch) {
            vec![Access {
                kind: crate::AccessKind::ChangeEvent {
                    id: data.arch_id,
                    component: self.component.key(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }

    fn describe(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "removed {}", self.component.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        self.component.searcher(searcher)
    }
}
