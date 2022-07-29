use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;

use super::{Slice, Slot};

#[derive(Default, Clone, PartialEq)]
/// A self compacting change tracking which holds either singular changes or a
/// range of changes, automatically merging adjacent ones.
///
///
/// The changes are always stored in a non-overlapping ascending order.
pub struct Changes {
    inner: Vec<Change>,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum ChangeKind {
    Modified,
    Inserted,
    Removed,
}

impl ChangeKind {
    /// Returns `true` if the change kind is [`Remove`].
    ///
    /// [`Remove`]: ChangeKind::Remove
    #[must_use]
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Returns `true` if the change kind is [`Insert`].
    ///
    /// [`Insert`]: ChangeKind::Insert
    #[must_use]
    pub fn is_inserted(&self) -> bool {
        matches!(self, Self::Inserted)
    }

    /// Returns `true` if the change kind is [`Change`].
    ///
    /// [`Change`]: ChangeKind::Change
    #[must_use]
    pub fn is_modified(&self) -> bool {
        matches!(self, Self::Modified)
    }

    pub(crate) fn is_modified_or_inserted(&self) -> bool {
        self.is_modified() || self.is_inserted()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct Change {
    pub slice: Slice,
    pub tick: u32,
    pub kind: ChangeKind,
}

impl Change {
    pub fn new(slice: Slice, tick: u32, kind: ChangeKind) -> Self {
        Self { slice, tick, kind }
    }

    pub fn modified(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Modified,
        }
    }

    pub fn inserted(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Inserted,
        }
    }

    pub fn removed(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Removed,
        }
    }
}

impl std::fmt::Debug for Changes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.inner).finish()
    }
}

impl Changes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_set(&self, f: impl Fn(&Change) -> bool) -> BTreeSet<Slot> {
        self.iter()
            .filter_map(|v| if f(v) { Some(v.slice) } else { None })
            .flatten()
            .collect()
    }

    pub fn as_map(&self) -> BTreeMap<Slot, (u32, ChangeKind)> {
        self.inner
            .iter()
            .flat_map(|v| v.slice.iter().map(move |p| (p, (v.tick, v.kind))))
            .collect()
    }

    pub fn set(&mut self, change: Change) -> &mut Self {
        tracing::debug!("Setting change: {change:?}");
        let mut insert_point = 0;
        let mut i = 0;
        let mut joined = false;

        self.inner.retain_mut(|v| {
            if v.tick < change.tick && v.kind == change.kind {
                if let Some(diff) = v.slice.difference(&change.slice) {
                    v.slice = diff;
                }
            }

            if v.tick == change.tick && v.kind == change.kind {
                // Merge atop change of the same change
                if let Some(u) = v.slice.union(&change.slice) {
                    joined = true;
                    v.slice = u;
                }
            }

            if v.slice.is_empty() {
                false
            } else if v.slice.start < change.slice.start {
                insert_point += 1;
                true
            } else {
                i += 1;
                true
            }
        });

        if !joined {
            self.inner.insert(insert_point, change);
        }

        debug_assert_eq!(
            self.inner
                .iter()
                .copied()
                .sorted_by_key(|v| v.slice.start)
                .collect_vec(),
            self.inner
        );

        tracing::debug!("Changes: {self:#?}");

        self
    }

    pub fn migrate_to(&mut self, other: &mut Self, src_slot: Slot, dst_slot: Slot) {
        for mut removed in self.remove(src_slot) {
            removed.slice = Slice::single(dst_slot);
            other.set(removed);
        }
    }

    /// Removes a slot from the change list
    pub fn remove(&mut self, slot: Slot) -> Vec<Change> {
        let slice = Slice::single(slot);
        let mut result = Vec::new();

        let removed = self
            .inner
            .drain(..)
            .flat_map(|v| {
                if let Some((l, _, r)) = v.slice.split_with(&slice) {
                    if !l.is_empty() {
                        result.push(Change::new(l, v.tick, v.kind));
                    }
                    if !r.is_empty() {
                        result.push(Change::new(r, v.tick, v.kind));
                    }

                    Some(Change::new(slice, v.tick, v.kind))
                } else {
                    result.push(v);
                    None
                }
            })
            .collect_vec();

        self.inner = result;
        removed
    }

    /// Returns the changes in the change list at a particular index.
    pub fn get(&self, index: usize) -> Option<&Change> {
        self.inner.get(index)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Iterate all changes in ascending order
    pub fn iter(&self) -> std::slice::Iter<Change> {
        self.inner.iter()
    }

    pub fn as_changed_set(&self, tick: u32) -> BTreeSet<Slot> {
        self.as_set(|v| v.kind.is_modified_or_inserted() && v.tick > tick)
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;
    #[test]
    fn changes() {
        let mut changes = Changes::new();

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
        let mut changes = Changes::new();

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
        let mut changes = Changes::new();

        changes.set(Change::modified(Slice::new(0, 63), 1));
        changes.set(Change::modified(Slice::new(63, 182), 1));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [Change::modified(Slice::new(0, 182), 1)]
        );
    }

    #[test]
    fn migrate() {
        let mut changes_1 = Changes::new();
        let mut changes_2 = Changes::new();

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
