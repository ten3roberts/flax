use core::fmt::Formatter;
use core::ops::Deref;

use alloc::vec::Vec;
use alloc::{string::ToString, vec};
use atomic_refcell::{AtomicRef, AtomicRefCell};

use crate::{
    archetype::{ChangeList, Slice},
    filter::PreparedFilter,
    Access, Archetype, ArchetypeId, ChangeKind, Component, ComponentValue, Fetch, FetchItem,
    Filter,
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
    const HAS_FILTER: bool = true;

    type Filter = Self;

    type Prepared = <Component<T> as Fetch<'w>>::Prepared;

    fn prepare(&'w self, data: crate::fetch::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        self.component.prepare(data)
    }

    fn matches(&self, data: crate::fetch::FetchPrepareData) -> bool {
        self.component.matches(data)
    }

    fn access(&self, data: crate::fetch::FetchPrepareData) -> Vec<Access> {
        self.component.access(data)
    }

    fn describe(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(&self.kind.to_string())?;
        match f.write_str(" ") {
            Ok(it) => it,
            Err(err) => return Err(err),
        };
        self.component.describe(f)
    }

    fn filter(&self) -> Self::Filter {
        Self {
            component: self.component,
            kind: self.kind,
        }
    }

    fn components(&self, result: &mut Vec<crate::ComponentKey>) {
        result.push(self.component.key())
    }
}

impl<'a, T: ComponentValue> Filter<'a> for ChangeFilter<T> {
    type Prepared = PreparedKindFilter<AtomicRef<'a, ChangeList>>;

    fn prepare(&'a self, arch: &'a Archetype, change_tick: u32) -> Self::Prepared {
        let changes = arch.changes(self.component.key());

        let changes = if let Some(changes) = changes {
            // Make sure to enable modification tracking if it is actively used
            if self.kind.is_modified() {
                changes.set_track_modified()
            }

            AtomicRef::map(changes, |changes| changes.get(self.kind))
        } else {
            EMPTY_CHANGELIST_CELL.borrow()
        };

        PreparedKindFilter::new(changes, change_tick)
    }

    fn matches(&self, archetype: &Archetype) -> bool {
        archetype.changes(self.component.key()).is_some()
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        if Filter::matches(self, archetype) {
            vec![Access {
                kind: crate::AccessKind::ChangeEvent {
                    id,
                    component: self.component.key(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }

    fn describe(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} {}", self.kind, self.component.name())
    }
}

#[derive(Debug)]
#[doc(hidden)]
pub struct PreparedKindFilter<A> {
    changes: A,
    cur: Option<Slice>,
    // The current change group.
    // Starts at the end and decrements
    cursor: usize,
    tick: u32,
}

impl<A> PreparedKindFilter<A>
where
    A: Deref<Target = ChangeList>,
{
    pub(crate) fn new(changes: A, tick: u32) -> Self {
        Self {
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

        // match (self.cur, self.changes.as_mut()) {
        //     (Some(v), _) => Some(v),
        //     (None, changes) => loop {
        //         let v = changes.get(self.cursor);
        //         if let Some(change) = v {
        //             self.cursor += 1;
        //             // Found a valid change slice
        //             if change.tick > self.tick {
        //                 break Some(*self.cur.get_or_insert(change.slice));
        //             }
        //         } else {
        //             // No more
        //             return None;
        //         };
        //     },
        //     _ => None,
        // }
    }
}

impl<A> PreparedFilter for PreparedKindFilter<A>
where
    A: Deref<Target = ChangeList>,
{
    fn filter(&mut self, slots: Slice) -> Slice {
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

    fn matches_slot(&mut self, slot: usize) -> bool {
        self.changes
            .iter()
            .any(|change| change.tick > self.tick && change.slice.contains(slot))
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

impl<'w, T: ComponentValue> Fetch<'w> for RemovedFilter<T> {
    const MUTABLE: bool = false;
    const HAS_FILTER: bool = true;

    type Filter = Self;

    type Prepared = ();

    fn prepare(&self, _: crate::fetch::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(())
    }

    fn matches(&self, _: crate::fetch::FetchPrepareData) -> bool {
        true
    }

    fn access(&self, _: crate::fetch::FetchPrepareData) -> Vec<Access> {
        Default::default()
    }

    fn describe(&self, f: &mut Formatter) -> core::fmt::Result {
        f.write_str(&ChangeKind::Removed.to_string())?;
        f.write_str(" ")?;
        self.component.describe(f)
    }

    fn filter(&self) -> Self::Filter {
        Self {
            component: self.component,
        }
    }

    fn components(&self, _: &mut Vec<crate::ComponentKey>) {}
}

impl<'a, T: ComponentValue> Filter<'a> for RemovedFilter<T> {
    type Prepared = PreparedKindFilter<&'a ChangeList>;

    fn prepare(&self, arch: &'a Archetype, change_tick: u32) -> Self::Prepared {
        let changes = arch
            .removals(self.component.key())
            .unwrap_or(&EMPTY_CHANGELIST);

        PreparedKindFilter::new(changes, change_tick)
    }

    fn matches(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        if Filter::matches(self, archetype) {
            vec![Access {
                kind: crate::AccessKind::ChangeEvent {
                    id,
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
}
