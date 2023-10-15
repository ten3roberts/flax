use core::{
    fmt::{self, Display, Formatter},
    ops::{Deref, DerefMut},
    sync::{self, atomic::AtomicBool},
};

use alloc::vec::Vec;

use itertools::Itertools;
use smallvec::SmallVec;

use super::{Slice, Slot};

#[derive(Default, Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
pub struct ChangeList {
    inner: Vec<Change>,
}

impl ChangeList {
    pub(crate) const fn new() -> Self {
        Self { inner: Vec::new() }
    }

    #[cfg(feature = "internal_assert")]
    fn assert_normal(&self, msg: &str) {
        let ordered = self
            .iter()
            .sorted_by_key(|v| v.slice.start)
            .copied()
            .collect_vec();

        if ordered != self.inner {
            panic!("Not ordered {self:#?}\nexpected: {ordered:#?}\n\n{msg}");
        }

        self.iter().for_each(|v| {
            assert!(!v.slice.is_empty(), "Slice {v:?} is empty: {self:#?} {msg}");
            assert!(
                v.slice.start < v.slice.end,
                "Slice {v:?} {self:#?} is inverted: {msg}"
            );
        })
    }

    pub(crate) fn set(&mut self, mut change: Change) -> &mut Self {
        let mut insert_point = 0;
        let mut i = 0;
        let mut joined = false;

        #[cfg(feature = "internal_assert")]
        self.assert_normal("Not sorted at beginning");

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
        self.assert_normal(&alloc::format!(
            "Not sorted after `set` inserting: {change:?}"
        ));

        self
    }

    #[cfg(test)]
    pub(crate) fn swap_remove_to(
        &mut self,
        src_slot: Slot,
        last: Slot,
        dst: &mut Self,
        dst_slot: Slot,
    ) {
        self.swap_remove_with(src_slot, last, |mut v| {
            // Change the slot
            v.slice = Slice::single(dst_slot);
            dst.set(v);
        })
    }

    #[cfg(test)]
    pub(crate) fn swap_remove_collect(&mut self, slot: Slot, last: Slot) -> Vec<Change> {
        let mut res = Vec::new();
        self.swap_remove_with(slot, last, |v| res.push(v));
        res
    }

    // Swap removes slot with the last slot
    // The supplied slot must be the >= all other stored slots
    pub(crate) fn swap_remove_with(
        &mut self,
        slot: Slot,
        last: Slot,
        mut on_removed: impl FnMut(Change),
    ) {
        #[cfg(feature = "internal_assert")]
        self.assert_normal(&format!("Invalid before swap remove: {slot}, last: {last}"));
        // self.swap_out(slot, last).into_iter().for_each(on_removed);
        // return;

        #[cfg(feature = "internal_assert")]
        assert!(
            self.iter().all(|v| v.slice.end <= last + 1),
            "last: {last}, {self:#?}"
        );

        if self.is_empty() {
            return;
        }

        // No swapping needed
        if slot == last {
            return self.remove(slot, on_removed);
        }

        // Pop off the changes from the very end
        let mut last_changes: SmallVec<[_; 8]> = self
            .iter_mut()
            .filter(|v| v.slice.contains(last))
            .map(|v| {
                v.slice.end = last;
                Change::single(slot, v.tick)
            })
            .collect();

        let start = self.iter().position(|v| v.slice.contains(slot));

        let end = self.iter().positions(|v| v.slice.contains(slot)).last();

        let (end, src) = match (start, end) {
            (Some(start), Some(end)) => {
                debug_assert!(start <= end, "{start}..{end}");
                (end, &mut self[start..=end])
            }
            (None, None) => (0, &mut self[0..0]),
            _ => {
                unreachable!()
            }
        };

        // Depending on if the last slot has a change at the same tick we either change the slot,
        // or split the change in three parts.
        //
        // Order is kept
        let mut split = SmallVec::<[_; 8]>::new();

        for change in src {
            on_removed(Change::single(slot, change.tick));

            if let Some(index) = last_changes.iter().position(|&v| v.tick == change.tick) {
                // The whole change is valid, even though the meaning of `slot` changed
                last_changes.swap_remove(index);
            } else {
                // This change needs to be split in two parts, with slot inbetween
                let slice = change.slice;

                debug_assert!(slice.contains(slot), "slice: {slice:?}, slot: {slot}");

                let l = Change::new(Slice::new(slice.start, slot), change.tick);

                debug_assert!(slot < slice.end, "slot: {slot}, slice: {slice:?}");
                let r = Change::new(Slice::new(slot + 1, slice.end), change.tick);

                // Order is still kept if change is replaced with `l`
                *change = l;

                if !r.slice.is_empty() {
                    split.push(r);
                }
            }
        }

        // all changes inside the slice have now been kept or overwritten
        if !split.is_empty() {
            let index = end + 1;
            self.splice(index..index, last_changes.into_iter().chain(split));
        }

        self.retain(|v| !v.slice.is_empty());
        #[cfg(feature = "internal_assert")]
        self.assert_normal(&format!(
            "Not sorted after `swap_remove` while removing: {slot}"
        ));

        self.iter()
            .for_each(|v| assert!(v.slice.start <= v.slice.end));
    }

    /// Removes a slot from the change list
    pub(crate) fn remove(&mut self, slot: Slot, mut on_removed: impl FnMut(Change)) {
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
        self.assert_normal("Not sorted before `remove`");

        self.inner.drain(..).for_each(|v| {
            if let Some((l, _, r)) = v.slice.split_with(&slice) {
                if !l.is_empty() {
                    // If the pending elements are smaller, push them first
                    if let Some(r) = right.first() {
                        if r.slice < l {
                            result.append(&mut right);
                        }
                    }

                    result.push(Change::new(l, v.tick));
                }
                if !r.is_empty() {
                    right.push(Change::new(r, v.tick));
                }

                on_removed(Change::new(slice, v.tick))
            } else {
                // If the pending elements are smaller, push them first
                if let Some(r) = right.first() {
                    if r.slice < v.slice {
                        result.append(&mut right);
                    }
                }

                result.push(v);
            }
        });

        result.append(&mut right);

        self.inner = result;
        #[cfg(feature = "internal_assert")]
        self.assert_normal(&alloc::format!(
            "Not sorted after `remove` while removing: {slot}"
        ));
    }

    pub fn iter_collapsed(&self) -> impl Iterator<Item = (Slot, u32)> + '_ {
        self.inner.iter().flat_map(|v| {
            let tick = v.tick;
            v.slice.iter().map(move |slot| (slot, tick))
        })
    }

    #[cfg(test)]
    pub(crate) fn as_changed_set(&self, tick: u32) -> alloc::collections::BTreeSet<Slot> {
        self.as_set(|v| v.tick > tick)
    }

    #[cfg(test)]
    pub(crate) fn as_set(&self, f: impl Fn(&Change) -> bool) -> alloc::collections::BTreeSet<Slot> {
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
#[doc(hidden)]
pub enum ChangeKind {
    /// Component was modified
    Modified = 0,
    /// Component was added
    Added = 1,
    /// Component was removed
    Removed = 2,
}

impl Display for ChangeKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ChangeKind::Modified => f.write_str("modified"),
            ChangeKind::Added => f.write_str("inserted"),
            ChangeKind::Removed => f.write_str("removed"),
        }
    }
}

impl ChangeKind {
    ///// Returns `true` if the change kind is [`Remove`].
    /////
    ///// [`Remove`]: ChangeKind::Removed
    //#[must_use]
    //pub fn is_removed(&self) -> bool {
    //    matches!(self, Self::Removed)
    //}

    ///// Returns `true` if the change kind is [`Insert`].
    /////
    ///// [`Insert`]: ChangeKind::Inserted
    //#[must_use]
    //pub fn is_inserted(&self) -> bool {
    //    matches!(self, Self::Inserted)
    //}

    /// Returns `true` if the change kind is [`ChangeKind::Modified`]
    ///
    /// [`Modified`]: ChangeKind::Modified
    #[must_use]
    pub fn is_modified(&self) -> bool {
        matches!(self, Self::Modified)
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
}

impl Change {
    /// Creates a new change
    pub(crate) fn new(slice: Slice, tick: u32) -> Self {
        Self { slice, tick }
    }
    #[inline]
    pub(crate) fn single(slot: Slot, tick: u32) -> Self {
        Self::new(Slice::new(slot, slot + 1), tick)
    }
}

/// A self compacting change tracking which holds either singular changes or a
/// range of changes, automatically merging adjacent ones.
///
///
/// The changes are always stored in a non-overlapping ascending order.
pub(crate) struct Changes {
    map: [ChangeList; 3],
    track_modified: AtomicBool,
}

impl Changes {
    pub(crate) fn new() -> Self {
        Self {
            track_modified: AtomicBool::new(false),
            map: Default::default(),
        }
    }

    #[inline]
    pub(crate) fn get(&self, kind: ChangeKind) -> &ChangeList {
        &self.map[kind as usize]
    }

    #[inline]
    pub(crate) fn set_added(&mut self, change: Change) -> &mut Self {
        self.map[ChangeKind::Added as usize].set(change);
        self.map[ChangeKind::Modified as usize].set(change);
        self
    }

    #[inline]
    pub(crate) fn set_modified_if_tracking(&mut self, change: Change) -> &mut Self {
        if self.track_modified() {
            self.set_modified(change);
        }

        self
    }

    #[inline]
    pub(crate) fn set(&mut self, kind: ChangeKind, change: Change) -> &mut Self {
        self.map[kind as usize].set(change);
        self
    }

    #[inline]
    pub(crate) fn set_modified(&mut self, change: Change) -> &mut Self {
        self.map[ChangeKind::Modified as usize].set(change);
        self
    }

    /// Removes `src` by swapping `dst` into its place
    pub(crate) fn swap_remove(
        &mut self,
        slot: Slot,
        dst: Slot,
        mut on_removed: impl FnMut(ChangeKind, Change),
    ) {
        self.map[0].swap_remove_with(slot, dst, |v| on_removed(ChangeKind::Modified, v));
        self.map[1].swap_remove_with(slot, dst, |v| on_removed(ChangeKind::Added, v));
        self.map[2].swap_remove_with(slot, dst, |v| on_removed(ChangeKind::Removed, v));
    }

    #[inline(always)]
    pub(crate) fn zip_map(
        &mut self,
        other: &mut Self,
        mut f: impl FnMut(ChangeKind, &mut ChangeList, &mut ChangeList),
    ) {
        f(ChangeKind::Modified, &mut self.map[0], &mut other.map[0]);
        f(ChangeKind::Added, &mut self.map[1], &mut other.map[1]);
        f(ChangeKind::Removed, &mut self.map[2], &mut other.map[2]);
    }

    pub(crate) fn set_track_modified(&self) {
        self.track_modified
            .store(true, sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn track_modified(&self) -> bool {
        self.track_modified.load(sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn clear(&mut self) {
        self.map[0].clear();
        self.map[1].clear();
        self.map[2].clear();
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;

    #[test]
    fn changes() {
        let mut changes = ChangeList::default();

        changes.set(Change::new(Slice::new(0, 5), 1));

        changes.set(Change::new(Slice::new(70, 92), 2));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::new(Slice::new(0, 5), 1),
                Change::new(Slice::new(70, 92), 2)
            ]
        );

        changes.set(Change::new(Slice::new(3, 5), 3));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::new(Slice::new(0, 3), 1),
                Change::new(Slice::new(3, 5), 3),
                Change::new(Slice::new(70, 92), 2),
            ]
        );

        // Extend previous change
        changes.set(Change::new(Slice::new(4, 14), 3));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::new(Slice::new(0, 3), 1),
                Change::new(Slice::new(3, 14), 3),
                Change::new(Slice::new(70, 92), 2),
            ]
        );

        // Overwrite almost all
        changes.set(Change::new(Slice::new(0, 89), 4));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::new(Slice::new(0, 89), 4),
                Change::new(Slice::new(89, 92), 2),
            ]
        );
    }

    #[test]
    fn changes_small() {
        let mut changes = ChangeList::default();

        for i in 0..239 {
            let perm = (i * (i + 2)) % 300;
            // let perm = i;
            changes.set(Change::new(Slice::single(perm), i as _));
        }

        changes.set(Change::new(Slice::new(70, 249), 300));
        changes.set(Change::new(Slice::new(0, 89), 301));
        changes.set(Change::new(Slice::new(209, 300), 302));
    }

    #[test]
    fn adjacent() {
        let mut changes = ChangeList::default();

        changes.set(Change::new(Slice::new(0, 63), 1));
        changes.set(Change::new(Slice::new(63, 182), 1));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [Change::new(Slice::new(0, 182), 1)]
        );
    }

    #[test]
    fn swap_remove_to() {
        let mut changes_1 = ChangeList::default();
        let mut changes_2 = ChangeList::default();

        changes_1
            .set(Change::new(Slice::new(20, 48), 1))
            .set(Change::new(Slice::new(32, 98), 2));

        assert_eq!(
            changes_1.inner,
            [
                Change::new(Slice::new(20, 32), 1),
                Change::new(Slice::new(32, 98), 2)
            ]
        );

        changes_1.swap_remove_to(25, 97, &mut changes_2, 67);

        assert_eq!(
            changes_1.inner,
            [
                Change::new(Slice::new(20, 25), 1),
                Change::new(Slice::new(25, 26), 2),
                Change::new(Slice::new(26, 32), 1),
                Change::new(Slice::new(32, 97), 2)
            ]
        );

        assert_eq!(changes_2.inner, [Change::new(Slice::single(67), 1)])
    }

    #[test]
    fn swap_remove() {
        let mut changes = ChangeList::default();

        changes.set(Change::new(Slice::new(4, 7), 6));
        changes.set(Change::new(Slice::new(1, 4), 2));
        changes.set(Change::new(Slice::new(1, 3), 8));
        changes.set(Change::new(Slice::new(5, 6), 1));
        // changes.swap_remove(1);
        assert_eq!(changes.swap_remove_collect(6, 6), [Change::single(6, 6)]);
        assert_eq!(changes.swap_remove_collect(6, 6), []);
        assert_eq!(changes.swap_remove_collect(1, 5), [Change::single(1, 8)]);

        changes.set(Change::new(Slice::new(3, 7), 3));
        changes.set(Change::new(Slice::new(3, 4), 5));

        // dbg!(&changes);
        assert_eq!(
            changes.swap_remove_collect(4, 9),
            [Change::single(4, 3), Change::single(4, 6)]
        );

        assert_eq!(changes.swap_remove_collect(4, 9), []);
    }
}
