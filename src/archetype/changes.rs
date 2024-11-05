use core::{
    fmt::{self, Display, Formatter},
    sync::{self, atomic::AtomicBool},
};

use alloc::vec::Vec;

use super::{Remainder, Slice, Slot};

#[derive(Default, Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
// Contains ranges of changes for the same change tick
// Sorted by the start range of the slices
//
// Adjacent of the same tick are merged together
pub struct ChangeList {
    pub(crate) inner: Vec<Change>,
}

impl ChangeList {
    // #[cfg(debug_assertions)]
    // fn assert_normal(&self, msg: &str) {
    //     let this = self.iter().flat_map(|v| v.slice).collect_vec();
    //     let ordered = self.iter().flat_map(|v| v.slice).dedup().collect_vec();

    //     if ordered != this {
    //         panic!("Not ordered {self:#?}\nexpected: {ordered:#?}\n\n{msg}");
    //     }

    //     self.iter().for_each(|v| {
    //         assert!(!v.slice.is_empty(), "Slice {v:?} is empty: {self:#?} {msg}");
    //         assert!(
    //             v.slice.start < v.slice.end,
    //             "Slice {v:?} {self:#?} is inverted: {msg}"
    //         );
    //     })
    // }

    fn merge_from(&mut self, mut i: usize) {
        let changes = &mut self.inner;
        let Change { mut slice, tick } = changes[i];

        // Merge forward
        while let Some(next) = changes.get_mut(i + 1) {
            if next.tick == tick {
                if let Some(u) = slice.union(&next.slice) {
                    slice = u;
                    changes[i].slice = u;
                    changes.remove(i + 1);
                    continue;
                }
            }

            if let Some(diff) = next.slice.difference(slice) {
                assert!(diff.start >= next.slice.start);
                next.slice = diff;
                if diff.is_empty() {
                    changes.remove(i + 1);
                    continue;
                }
            }

            i += 1;
        }
    }

    pub(crate) fn set(&mut self, mut new_change: Change) -> &mut Self {
        // let orig = self.inner.clone();
        let mut insert_point = 0;
        let mut i = 0;

        // #[cfg(debug_assertions)]
        // self.assert_normal("Not sorted before");

        let changes = &mut self.inner;

        while i < changes.len() {
            let change = &mut changes[i];
            let slice = &mut change.slice;

            if slice.start < new_change.slice.start {
                // TODO: break
                insert_point = i + 1;
            }

            // Merge
            match change.tick.cmp(&new_change.tick) {
                // Remove the incoming changes range from the existing ones
                core::cmp::Ordering::Less => {
                    // Remove overlaps with existing intervals of previous ticks
                    match slice.subtract(&new_change.slice) {
                        Remainder::NoOverlap => {
                            i += 1;
                        }
                        Remainder::FullOverlap => {
                            // eprintln!("Removing {i} {change:?}");
                            changes.remove(i);
                        }
                        Remainder::Left(l) => {
                            // eprintln!("{slice:?} => {l:?}");
                            change.slice = l;
                            i += 1;
                        }
                        Remainder::Right(r) => {
                            // eprintln!("{slice:?} => {r:?}");
                            change.slice = r;
                            i += 1;
                        }
                        Remainder::Split(l, r) => {
                            // eprintln!("{slice:?} => {l:?}, {l:?}");
                            change.slice = l;
                            let tick = change.tick;
                            changes.insert(i + 1, Change::new(r, tick));
                            i += 2;
                        }
                    }
                }
                core::cmp::Ordering::Equal => {
                    // Attempt to merge
                    if let Some(union) = slice.union(&new_change.slice) {
                        change.slice = union;
                        // eprintln!("Merge: {slice:?} {value:?} => {change:?}");

                        // Merge forward
                        self.merge_from(i);

                        // #[cfg(debug_assertions)]
                        // self.assert_normal(&alloc::format!(
                        //     "Not sorted after `set` inserting: {value:?}"
                        // ));

                        return self;
                    }

                    i += 1;
                }
                // Existing changes are later, don't overwrite
                core::cmp::Ordering::Greater => match new_change.slice.subtract(&change.slice) {
                    Remainder::NoOverlap => {
                        i += 1;
                    }
                    Remainder::FullOverlap => {
                        // nothing to be done
                        return self;
                    }
                    Remainder::Left(left) => {
                        new_change.slice = left;
                        i += 1;
                    }
                    Remainder::Right(right) => {
                        new_change.slice = right;
                        i += 1;
                    }
                    Remainder::Split(left, right) => {
                        new_change.slice = left;

                        let tick = new_change.tick;
                        changes.insert(i + 1, Change::new(right, tick));
                        i += 2;
                    }
                },
            }
        }

        self.inner.insert(insert_point, new_change);

        // #[cfg(debug_assertions)]
        // self.assert_normal(&alloc::format!(
        //     "Not sorted after `set` inserting: {value:?}\n\noriginal: {orig:?}"
        // ));

        self
    }

    pub(crate) fn set_slot(&mut self, slot: Slot, tick: u32) -> &mut Self {
        let mut insert_point = 0;
        let mut i = 0;

        // #[cfg(debug_assertions)]
        // self.assert_normal("Not sorted at beginning");

        let changes = &mut self.inner;

        while i < changes.len() {
            let change = &mut changes[i];
            let slice = change.slice;

            if slice.start < slot {
                insert_point = i + 1;
            }

            // Merge
            match change.tick.cmp(&tick) {
                // Remove the incoming changes range from the existing ones
                core::cmp::Ordering::Less => {
                    // Remove overlaps with existing intervals of previous ticks
                    match slice.subtract(&Slice::single(slot)) {
                        Remainder::NoOverlap => {
                            i += 1;
                        }
                        Remainder::FullOverlap => {
                            // eprintln!("Removing {i} {change:?}");
                            changes.remove(i);
                        }
                        Remainder::Left(l) => {
                            // eprintln!("{slice:?} => {l:?}");
                            change.slice = l;
                            i += 1;
                        }
                        Remainder::Right(r) => {
                            // eprintln!("{slice:?} => {r:?}");
                            change.slice = r;
                            i += 1;
                        }
                        Remainder::Split(l, r) => {
                            // eprintln!("{slice:?} => {l:?}, {l:?}");
                            change.slice = l;
                            let tick = change.tick;
                            changes.insert(i + 1, Change::new(r, tick));
                            i += 2;
                        }
                    }
                }
                core::cmp::Ordering::Equal => {
                    // Attempt to merge
                    if slice.start <= slot && slice.end >= slot {
                        change.slice = Slice::new(slice.start, (slot + 1).max(slice.end));

                        // eprintln!("Merge: {slice:?} {slot:?} => {change:?}");

                        self.merge_from(i);

                        // #[cfg(debug_assertions)]
                        // self.assert_normal(&alloc::format!(
                        //     "Not sorted after `set` inserting: {slot:?}"
                        // ));

                        return self;
                    }

                    i += 1;
                }
                core::cmp::Ordering::Greater => {
                    if slice.contains(slot) {
                        return self;
                    }
                    i += 1;
                }
            }
        }

        self.inner
            .insert(insert_point, Change::new(Slice::single(slot), tick));

        // #[cfg(debug_assertions)]
        // self.assert_normal(&alloc::format!(
        //     "Not sorted after `set_slot` inserting: {slot:?}"
        // ));

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

    pub(crate) fn swap_remove_with(
        &mut self,
        slot: Slot,
        swap: Slot,
        mut on_removed: impl FnMut(Change),
    ) {
        let mut to_swap = None;
        let orig = self.inner.clone();

        // Truncate all ranges from the swapped slot
        if slot != swap {
            self.inner.retain_mut(|v| {
                // assert_eq!(v.slice.end, swap + 1);
                // 0 or more in the tail may become empty
                if v.slice.end == swap + 1 {
                    v.slice.end = swap;
                    assert!(
                        to_swap.is_none(),
                        "Multiple changes for the same tick {slot} {swap} {orig:?}"
                    );
                    to_swap = Some((slot, v.tick));
                }

                !v.slice.is_empty()
            });
        }

        let mut i = 0;
        let changes = &mut self.inner;

        while i < changes.len() {
            let change = &mut changes[i];

            let slice = change.slice;
            if slice.start > slot {
                break;
            } else if slice.end <= slot {
                // phew, not containing the slot, skip
                i += 1;
                continue;
            }

            on_removed(Change::single(slot, change.tick));

            // We need to handle this range

            // There is a change for the same tick, so we can substitute directly
            if to_swap.is_some_and(|v| v.1 == change.tick) {
                to_swap = None;
                i += 1;
                continue;
            }

            // There was no change in the same tick for the swapped in slot, so we need to fix
            // this slice instead

            // We need to handle this range
            //
            // Easy: Truncate from start
            if slice.start == slot {
                if slice.end > slot + 1 {
                    change.slice.start = slot + 1;
                    i += 1;
                } else {
                    assert_eq!(slice.len(), 1);
                    // Empty, remove
                    changes.remove(i);
                    i += 1;
                }
            }
            // Truncate end
            else if slice.end == slot + 1 {
                // From above we know that start != slot
                // dst.push(Change {
                //     tick: change.tick,
                //     slice: Slice::new(slice.start, slot),
                // })
                change.slice.end = slot;
                i += 1;
            }
            // Oh no, it is in the middle
            else {
                let left = Change {
                    tick: change.tick,
                    slice: Slice::new(slice.start, slot),
                };
                let right = Change {
                    tick: change.tick,
                    slice: Slice::new(slot + 1, slice.end),
                };

                *change = left;
                changes.insert(i + 1, right);
                i += 2;
            }
        }

        if let Some((slot, tick)) = to_swap {
            self.set_slot(slot, tick);
        }
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
        self.inner
            .iter()
            .filter_map(|v| if f(v) { Some(v.slice) } else { None })
            .flatten()
            .collect()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, Change> {
        self.inner.iter()
    }

    pub fn as_slice(&self) -> &[Change] {
        self.inner.as_slice()
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
    pub(crate) fn set_slot(&mut self, kind: ChangeKind, slot: Slot, tick: u32) -> &mut Self {
        self.map[kind as usize].set_slot(slot, tick);
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
        self.map[0].inner.clear();
        self.map[1].inner.clear();
        self.map[2].inner.clear();
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
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

        changes.set(Change::new(Slice::new(5, 6), 1));
        changes.set(Change::new(Slice::new(1, 4), 2));
        changes.set(Change::new(Slice::new(4, 7), 6));
        changes.set(Change::new(Slice::new(1, 3), 8));

        assert_eq!(
            changes.as_slice(),
            [
                Change::new(Slice::new(1, 3), 8),
                Change::new(Slice::new(3, 4), 2),
                Change::new(Slice::new(4, 7), 6),
            ]
        );

        // changes.swap_remove(1);
        assert_eq!(changes.swap_remove_collect(6, 6), [Change::single(6, 6)]);
        assert_eq!(changes.swap_remove_collect(6, 6), []);
        assert_eq!(changes.swap_remove_collect(1, 5), [Change::single(1, 8)]);

        assert_eq!(
            changes.as_slice(),
            [
                Change::new(Slice::new(1, 2), 6),
                Change::new(Slice::new(2, 3), 8),
                Change::new(Slice::new(3, 4), 2),
                Change::new(Slice::new(4, 5), 6),
            ]
        );

        changes.set(Change::new(Slice::new(3, 7), 8));
        changes.set(Change::new(Slice::new(3, 4), 9));

        assert_eq!(
            changes.as_slice(),
            [
                Change::new(Slice::new(1, 2), 6),
                Change::new(Slice::new(2, 3), 8),
                Change::new(Slice::new(3, 4), 9),
                Change::new(Slice::new(4, 7), 8),
                // Change::new(Slice::new(3, 4), 2),
                // Change::new(Slice::new(4, 5), 6),
            ]
        );

        assert_eq!(changes.swap_remove_collect(4, 6), [Change::single(4, 8)]);

        assert_eq!(
            changes.as_slice(),
            [
                Change::new(Slice::new(1, 2), 6),
                Change::new(Slice::new(2, 3), 8),
                Change::new(Slice::new(3, 4), 9),
                Change::new(Slice::new(4, 6), 8),
                // Change::new(Slice::new(3, 4), 2),
                // Change::new(Slice::new(4, 5), 6),
            ]
        );

        assert_eq!(changes.swap_remove_collect(4, 9), [Change::single(4, 8)]);

        assert_eq!(
            changes.as_slice(),
            [
                Change::new(Slice::new(1, 2), 6),
                Change::new(Slice::new(2, 3), 8),
                Change::new(Slice::new(3, 4), 9),
                Change::new(Slice::new(5, 6), 8),
                // Change::new(Slice::new(3, 4), 2),
                // Change::new(Slice::new(4, 5), 6),
            ]
        );
    }

    #[test]
    fn insert() {
        let mut changes = ChangeList {
            inner: vec![
                Change::new(Slice::new(0, 2), 1),
                Change::new(Slice::new(2, 3), 2),
            ],
        };

        changes.set(Change::new(Slice::new(0, 3), 2));

        assert_eq!(changes.as_slice(), [Change::new(Slice::new(0, 3), 2),]);
    }

    #[test]
    fn insert2() {
        let mut changes = ChangeList {
            inner: vec![
                Change::new(Slice::new(0, 2), 1),
                Change::new(Slice::new(2, 3), 2),
            ],
        };

        changes.set_slot(0, 2);
        changes.set_slot(1, 2);
        changes.set_slot(2, 2);

        assert_eq!(changes.as_slice(), [Change::new(Slice::new(0, 3), 2),]);
    }
}
