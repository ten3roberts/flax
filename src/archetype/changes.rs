use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
    sync::atomic::AtomicBool,
};

use itertools::Itertools;

use crate::ComponentInfo;

use super::{Slice, Slot};

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeList {
    inner: Vec<Change>,
}

impl ChangeList {
    #[cfg(feature = "internal_assert")]
    fn assert_ordered(&self, msg: &str) {
        let ordered = self
            .iter()
            .sorted_by_key(|v| v.slice.start)
            .copied()
            .collect_vec();

        if ordered != self.inner {
            panic!("Not ordered {ordered:#?} found: {self:#?}\n\n{msg}");
        }
    }

    pub(crate) fn set(&mut self, mut change: Change) -> &mut Self {
        let mut insert_point = 0;
        let mut i = 0;
        let mut joined = false;

        #[cfg(feature = "internal_assert")]
        self.assert_ordered("Not sorted at beginning");

        self.inner.retain_mut(|v| {
            if change.slice.is_empty() {
                return true;
            }
            // Remove older changes which are a subset of the newer slots
            if v.tick < change.tick {
                if let Some(diff) = v.slice.difference(change.slice) {
                    v.slice = diff;
                }
            } else if let Some(diff) = change.slice.difference(v.slice) {
                change.slice = diff;
            }

            // Merge the change into an already existing change
            // Do not change start as that will invalidate ordering
            if v.slice < change.slice && v.tick == change.tick {
                // Merge atop change of the same change
                if let Some(u) = v.slice.union(&change.slice) {
                    joined = true;
                    v.slice = u;
                }
            }

            if v.slice.is_empty() {
                return false;
            }

            i += 1;

            if v.slice < change.slice {
                insert_point = i;
            }

            true
        });

        if !joined && !change.slice.is_empty() {
            self.inner.insert(insert_point, change);
        }

        #[cfg(feature = "internal_assert")]
        self.assert_ordered(&format!("Not sorted after `set` inserting: {change:?}"));

        self
    }

    pub(crate) fn migrate_to(&mut self, other: &mut Self, src_slot: Slot, dst_slot: Slot) {
        for mut removed in self.remove(src_slot) {
            // Change the slot
            removed.slice = Slice::single(dst_slot);
            other.set(removed);
        }
    }

    /// Removes `src` by swapping `dst` into its place
    pub(crate) fn swap_out(&mut self, src: Slot, dst: Slot) -> Vec<Change> {
        let src_changes = self.remove(src);
        let dst_changes = self.remove(dst);

        for mut v in dst_changes.into_iter() {
            assert_eq!(v.slice, Slice::single(dst));
            v.slice = Slice::single(src);
            self.set(v);
        }

        src_changes
    }

    /// Removes a slot from the change list
    pub fn remove(&mut self, slot: Slot) -> Vec<Change> {
        let slice = Slice::single(slot);
        let mut result = Vec::with_capacity(self.inner.capacity());

        let mut right: Vec<Change> = Vec::new();

        // =====-=====
        //    ==-=========
        //     =-===
        //
        // =====
        //    ==
        //     =
        //
        // right: ====, =========, ===

        // ====
        //   ==
        //    =
        //      ====
        //      =========
        //      ===

        #[cfg(feature = "internal_assert")]
        self.assert_ordered("Not sorted before `remove`");

        let removed = self
            .inner
            .drain(..)
            .flat_map(|v| {
                if let Some((l, _, r)) = v.slice.split_with(&slice) {
                    if !l.is_empty() {
                        // If the pending elements are smaller, push them first
                        if let Some(r) = right.first() {
                            if r.slice < l {
                                result.append(&mut right);
                            }
                        }

                        result.push(Change::new(l, v.tick, v.kind));
                    }
                    if !r.is_empty() {
                        right.push(Change::new(r, v.tick, v.kind));
                    }

                    Some(Change::new(slice, v.tick, v.kind))
                } else {
                    // If the pending elements are smaller, push them first
                    if let Some(r) = right.first() {
                        if r.slice < v.slice {
                            result.append(&mut right);
                        }
                    }

                    result.push(v);
                    None
                }
            })
            .collect_vec();

        result.append(&mut right);

        self.inner = result;
        #[cfg(feature = "internal_assert")]
        self.assert_ordered(&format!("Not sorted after `remove` while removing: {slot}"));
        removed
    }

    /// Returns the changes in the change list at a particular index.
    pub fn get(&self, index: usize) -> Option<&Change> {
        self.inner.get(index)
    }

    #[cfg(test)]
    pub(crate) fn as_changed_set(&self, tick: u32) -> std::collections::BTreeSet<Slot> {
        self.as_set(|v| v.kind.is_modified_or_inserted() && v.tick > tick)
    }

    #[cfg(test)]
    pub(crate) fn as_set(&self, f: impl Fn(&Change) -> bool) -> std::collections::BTreeSet<Slot> {
        self.iter()
            .filter_map(|v| if f(v) { Some(v.slice) } else { None })
            .flatten()
            .collect()
    }
}

impl Deref for ChangeList {
    type Target = Vec<Change>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for ChangeList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
/// Represents a change for a slice of entities for a specific component
pub enum ChangeKind {
    /// Component was modified
    Modified = 0,
    /// Component was inserted
    Inserted = 1,
    /// Component was removed
    Removed = 2,
}

impl Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeKind::Modified => f.write_str("modified"),
            ChangeKind::Inserted => f.write_str("inserted"),
            ChangeKind::Removed => f.write_str("removed"),
        }
    }
}

impl ChangeKind {
    /// Returns `true` if the change kind is [`Remove`].
    ///
    /// [`Remove`]: ChangeKind::Removed
    #[must_use]
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Returns `true` if the change kind is [`Insert`].
    ///
    /// [`Insert`]: ChangeKind::Inserted
    #[must_use]
    pub fn is_inserted(&self) -> bool {
        matches!(self, Self::Inserted)
    }

    /// Returns `true` if the change kind is [`ChangeKind::Modified`]
    ///
    /// [`Modified`]: ChangeKind::Modified
    #[must_use]
    pub fn is_modified(&self) -> bool {
        matches!(self, Self::Modified)
    }

    #[cfg(test)]
    pub(crate) fn is_modified_or_inserted(&self) -> bool {
        self.is_modified() || self.is_inserted()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
/// Represents a change over a slice of entities in an archetype which ocurred
/// at a specific time.
pub struct Change {
    /// The slice of entities in the archetype which are affected
    pub slice: Slice,
    /// The world tick of the change event
    pub tick: u32,
    /// The kind of change
    pub kind: ChangeKind,
}

impl Change {
    /// Creates a new change
    pub(crate) fn new(slice: Slice, tick: u32, kind: ChangeKind) -> Self {
        Self { slice, tick, kind }
    }

    /// Create a new modification event
    pub(crate) fn modified(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Modified,
        }
    }

    /// Create a new insert event
    pub(crate) fn inserted(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Inserted,
        }
    }

    /// Create a new remove event
    pub(crate) fn removed(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Removed,
        }
    }
}

#[derive(Debug)]
/// A self compacting change tracking which holds either singular changes or a
/// range of changes, automatically merging adjacent ones.
///
///
/// The changes are always stored in a non-overlapping ascending order.
pub struct Changes {
    info: ComponentInfo,

    map: [ChangeList; 3],
    track_modified: AtomicBool,
}

impl Changes {
    pub(crate) fn new(info: ComponentInfo) -> Self {
        Self {
            info,

            track_modified: AtomicBool::new(false),
            map: Default::default(),
        }
    }

    #[inline]
    pub(crate) fn get(&self, kind: ChangeKind) -> &ChangeList {
        &self.map[kind as usize]
    }

    pub(crate) fn set_inserted(&mut self, change: Change) -> &mut Self {
        self.map[ChangeKind::Inserted as usize].set(change);
        self.map[ChangeKind::Modified as usize].set(change);
        self
    }

    pub(crate) fn set_modified_if_tracking(&mut self, change: Change) -> &mut Self {
        if self.track_modified() {
            self.set_modified(change);
        }

        self
    }

    pub(crate) fn set_modified(&mut self, change: Change) -> &mut Self {
        self.map[ChangeKind::Modified as usize].set(change);
        self
    }

    pub(crate) fn set_removed(&mut self, change: Change) -> &mut Self {
        self.map[ChangeKind::Removed as usize].set(change);
        self
    }

    pub(crate) fn migrate_to(&mut self, other: &mut Self, src_slot: Slot, dst_slot: Slot) {
        for (a, b) in self.map.iter_mut().zip(other.map.iter_mut()) {
            a.migrate_to(b, src_slot, dst_slot)
        }
    }

    /// Removes `src` by swapping `dst` into its place
    pub(crate) fn swap_out(&mut self, src: Slot, dst: Slot) -> [Vec<Change>; 3] {
        [
            self.map[0].swap_out(src, dst),
            self.map[1].swap_out(src, dst),
            self.map[2].swap_out(src, dst),
        ]
    }

    /// Removes a slot from the change list
    pub fn remove(&mut self, slot: Slot) -> [Vec<Change>; 3] {
        [
            self.map[0].remove(slot),
            self.map[1].remove(slot),
            self.map[2].remove(slot),
        ]
    }

    pub(crate) fn info(&self) -> ComponentInfo {
        self.info
    }

    pub(crate) fn append_inserted(&mut self, changes: Vec<Change>) -> &mut Self {
        for v in changes {
            self.map[ChangeKind::Inserted as usize].set(v);
        }
        self
    }

    pub(crate) fn append_modified(&mut self, changes: Vec<Change>) -> &mut Self {
        for v in changes {
            self.map[ChangeKind::Modified as usize].set(v);
        }
        self
    }

    pub(crate) fn append_removed(&mut self, changes: Vec<Change>) -> &mut Self {
        for v in changes {
            self.map[ChangeKind::Removed as usize].set(v);
        }
        self
    }

    pub(crate) fn set_track_modified(&self) {
        self.track_modified
            .store(true, std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn track_modified(&self) -> bool {
        self.track_modified
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;

    #[test]
    fn changes() {
        let mut changes = ChangeList::default();

        changes.set(Change::modified(Slice::new(0, 5), 1));

        changes.set(Change::modified(Slice::new(70, 92), 2));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 5), 1),
                Change::modified(Slice::new(70, 92), 2)
            ]
        );

        changes.set(Change::modified(Slice::new(3, 5), 3));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 3), 1),
                Change::modified(Slice::new(3, 5), 3),
                Change::modified(Slice::new(70, 92), 2),
            ]
        );

        // Extend previous change
        changes.set(Change::modified(Slice::new(4, 14), 3));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 3), 1),
                Change::modified(Slice::new(3, 14), 3),
                Change::modified(Slice::new(70, 92), 2),
            ]
        );

        // Overwrite almost all
        changes.set(Change::modified(Slice::new(0, 89), 4));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 89), 4),
                Change::modified(Slice::new(89, 92), 2),
            ]
        );
    }

    #[test]
    fn changes_small() {
        let mut changes = ChangeList::default();

        for i in 0..239 {
            let perm = (i * (i + 2)) % 300;
            // let perm = i;
            changes.set(Change::modified(Slice::single(perm), i as _));
        }

        changes.set(Change::modified(Slice::new(70, 249), 300));
        changes.set(Change::modified(Slice::new(0, 89), 301));
        changes.set(Change::modified(Slice::new(209, 300), 302));

        eprintln!("Changes: {changes:#?}");
    }

    #[test]
    fn adjacent() {
        let mut changes = ChangeList::default();

        changes.set(Change::modified(Slice::new(0, 63), 1));
        changes.set(Change::modified(Slice::new(63, 182), 1));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [Change::modified(Slice::new(0, 182), 1)]
        );
    }

    #[test]
    fn migrate() {
        let mut changes_1 = ChangeList::default();
        let mut changes_2 = ChangeList::default();

        changes_1
            .set(Change::modified(Slice::new(20, 48), 1))
            .set(Change::modified(Slice::new(32, 98), 2));

        assert_eq!(
            changes_1.inner,
            [
                Change::modified(Slice::new(20, 32), 1),
                Change::modified(Slice::new(32, 98), 2)
            ]
        );

        changes_1.migrate_to(&mut changes_2, 25, 67);

        assert_eq!(
            changes_1.inner,
            [
                Change::modified(Slice::new(20, 25), 1),
                Change::modified(Slice::new(26, 32), 1),
                Change::modified(Slice::new(32, 98), 2)
            ]
        );

        assert_eq!(changes_2.inner, [Change::modified(Slice::single(67), 1)])
    }
}
