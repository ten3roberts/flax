use atomic_refcell::AtomicRef;

use crate::{
    archetype::{ChangeList, Slice},
    filter::PreparedFilter,
    Access, Archetype, ArchetypeId, ChangeKind, Component, ComponentValue, Fetch, FetchItem,
    Filter,
};

#[derive(Clone)]
/// Filter which only yields modified or inserted components
pub struct ChangeFilter<T: ComponentValue> {
    component: Component<T>,
    kind: ChangeKind,
}

impl<T: ComponentValue> std::fmt::Debug for ChangeFilter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

    fn difference(&self, data: crate::fetch::FetchPrepareData) -> Vec<String> {
        self.component.difference(data)
    }

    fn describe(&self, f: &mut dyn std::fmt::Write) -> core::fmt::Result {
        f.write_str(&self.kind.to_string())?;
        f.write_str(" ")?;
        self.component.describe(f)
    }

    fn filter(&self) -> Self::Filter {
        Self {
            component: self.component,
            kind: self.kind,
        }
    }
}

impl<'a, T: ComponentValue> Filter<'a> for ChangeFilter<T> {
    type Prepared = PreparedKindFilter<'a>;

    fn prepare(&'a self, arch: &'a Archetype, change_tick: u32) -> Self::Prepared {
        let changes = arch.changes(self.component.id());

        if let Some(ref changes) = changes && self.kind.is_modified() {
            changes.set_track_modified()
        }

        let changes = changes.map(|v| AtomicRef::map(v, |v| v.get(self.kind)));

        PreparedKindFilter::new(changes, change_tick)
    }

    fn matches(&self, archetype: &Archetype) -> bool {
        archetype.changes(self.component.id()).is_some()
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        if Filter::matches(self, archetype) {
            vec![Access {
                kind: crate::AccessKind::ChangeEvent {
                    id,
                    component: self.component.id(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }
}

#[derive(Debug)]
#[doc(hidden)]
pub struct PreparedKindFilter<'a> {
    changes: Option<AtomicRef<'a, ChangeList>>,
    cur: Option<Slice>,
    // The current change group.
    // Starts at the end and decrements
    index: usize,
    tick: u32,
}

impl<'a> PreparedKindFilter<'a> {
    pub(crate) fn new(changes: Option<AtomicRef<'a, ChangeList>>, tick: u32) -> Self {
        Self {
            changes,
            cur: None,
            index: 0,
            tick,
        }
    }

    pub fn current_slice(&mut self) -> Option<Slice> {
        match (self.cur, self.changes.as_mut()) {
            (Some(v), _) => Some(v),
            (None, Some(changes)) => loop {
                let v = changes.get(self.index);
                if let Some(change) = v {
                    self.index += 1;
                    if change.tick > self.tick {
                        break Some(*self.cur.get_or_insert(change.slice));
                    }
                } else {
                    // No more
                    return None;
                };
            },
            _ => None,
        }
    }
}

impl<'a> PreparedFilter for PreparedKindFilter<'a> {
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
}

#[derive(Clone)]
/// Filter which only yields removed `components
pub struct RemovedFilter<T: ComponentValue> {
    component: Component<T>,
}

impl<T: ComponentValue> std::fmt::Debug for RemovedFilter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

    fn prepare(&'w self, _: crate::fetch::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(())
    }

    fn matches(&self, _: crate::fetch::FetchPrepareData) -> bool {
        true
    }

    fn access(&self, _: crate::fetch::FetchPrepareData) -> Vec<Access> {
        vec![]
    }

    fn difference(&self, _: crate::fetch::FetchPrepareData) -> Vec<String> {
        vec![]
    }

    fn describe(&self, f: &mut dyn std::fmt::Write) -> core::fmt::Result {
        f.write_str(&ChangeKind::Removed.to_string())?;
        f.write_str(" ")?;
        self.component.describe(f)
    }

    fn filter(&self) -> Self::Filter {
        Self {
            component: self.component,
        }
    }
}
impl<'a, T: ComponentValue> Filter<'a> for RemovedFilter<T> {
    type Prepared = PreparedKindFilter<'a>;

    fn prepare(&self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        let changes = archetype
            .changes(self.component.id())
            .map(|v| AtomicRef::map(v, |v| v.get(ChangeKind::Removed)));

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
                    component: self.component.id(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }
}
