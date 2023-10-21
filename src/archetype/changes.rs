use core::{
    fmt::{self, Display, Formatter},
    sync::{self, atomic::AtomicBool},
};

use alloc::vec::Vec;

use itertools::Itertools;
use smallvec::SmallVec;

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

    fn merge_from(&mut self, mut i: usize) {
        let changes = &mut self.inner;
        let Change { mut slice, tick } = changes[i];
        dbg!(slice, tick);

        // Merge forward
        while let Some(next) = changes.get_mut(i + 1) {
            if next.tick == tick {
                if let Some(u) = slice.union(&next.slice) {
                    eprintln!("Merged forward in set {slice:?} {next:?} into {u:?}");
                    slice = u;
                    changes[i].slice = u;
                    changes.remove(i + 1);
                    continue;
                }
            }

            if let Some(diff) = next.slice.difference(slice) {
                eprintln!("Subtracting start {next:?} => {diff:?}");
                assert!(diff.start >= next.slice.start);
                next.slice = diff;
                if diff.is_empty() {
                    changes.remove(i + 1);
                    continue;
                }
            }

            i += 1;
        }

        eprintln!("Finished merging {self:?}");
    }

    pub(crate) fn set(&mut self, value: Change) -> &mut Self {
        eprintln!("set {value:?}");
        let mut insert_point = 0;
        let mut i = 0;

        #[cfg(feature = "internal_assert")]
        self.assert_normal("Not sorted before");

        let changes = &mut self.inner;

        while i < changes.len() {
            let change = &mut changes[i];
            let slice = change.slice;

            if slice.start < value.slice.start {
                // TODO: break
                insert_point = i + 1;
            }

            // Merge
            match change.tick.cmp(&value.tick) {
                // Remove the incoming changes range from the existing ones
                core::cmp::Ordering::Less => {
                    // Remove overlaps with existing intervals of previous ticks
                    match slice.subtract(&value.slice) {
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
                    if slice.start <= value.slice.start && value.slice.start <= slice.end {
                        change.slice = Slice::new(slice.start, value.slice.end.max(slice.end));
                        // eprintln!("Merge: {slice:?} {value:?} => {change:?}");

                        // Merge forward
                        self.merge_from(i);

                        #[cfg(feature = "internal_assert")]
                        self.assert_normal(&alloc::format!(
                            "Not sorted after `set` inserting: {value:?}"
                        ));

                        return self;
                    }

                    i += 1;
                }
                core::cmp::Ordering::Greater => unreachable!(),
            }
        }

        eprintln!("Insert at {insert_point}");
        self.inner.insert(insert_point, value);

        #[cfg(feature = "internal_assert")]
        self.assert_normal(&alloc::format!(
            "Not sorted after `set` inserting: {value:?}"
        ));

        eprintln!("After set: {self:?}");

        self
    }

    pub(crate) fn set_slot(&mut self, slot: Slot, tick: u32) -> &mut Self {
        eprintln!("set_slot {self:?} {slot} {tick}");
        let mut insert_point = 0;
        let mut i = 0;

        #[cfg(feature = "internal_assert")]
        self.assert_normal("Not sorted at beginning");

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

                        #[cfg(feature = "internal_assert")]
                        self.assert_normal(&alloc::format!(
                            "Not sorted after `set` inserting: {slot:?}"
                        ));

                        self.merge_from(i);

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

        #[cfg(feature = "internal_assert")]
        self.assert_normal(&alloc::format!(
            "Not sorted after `set` inserting: {slot:?}"
        ));

        eprintln!("After set_slot: {self:?}");

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

        // Truncate all ranges from the swapped slot
        if slot != swap {
            self.inner.retain_mut(|v| {
                // assert_eq!(v.slice.end, swap + 1);
                // 0 or more in the tail may become empty
                if v.slice.end == swap + 1 {
                    v.slice.end = swap;
                    assert!(to_swap.is_none());
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
                eprintln!("Substituting");
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
            eprintln!("Setting {slot} {tick}");
            self.set_slot(slot, tick);
        }
    }

    // Swap removes slot with the last slot
    // The supplied slot must be the >= all other stored slots
    pub(crate) fn swap_remove_with2(
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

        if self.inner.is_empty() {
            return;
        }

        // No swapping needed
        if slot == last {
            return self.remove(slot, on_removed);
        }

        // Pop off the changes from the very end
        let mut last_changes: SmallVec<[_; 8]> = self
            .inner
            .iter_mut()
            .filter(|v| v.slice.contains(last))
            .map(|v| {
                v.slice.end = last;
                Change::single(slot, v.tick)
            })
            .collect();

        let start = self.inner.iter().position(|v| v.slice.contains(slot));

        let end = self
            .inner
            .iter()
            .positions(|v| v.slice.contains(slot))
            .last();

        let (end, src) = match (start, end) {
            (Some(start), Some(end)) => {
                debug_assert!(start <= end, "{start}..{end}");
                (end, &mut self.inner[start..=end])
            }
            (None, None) => (0, &mut self.inner[0..0]),
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
            self.inner
                .splice(index..index, last_changes.into_iter().chain(split));
        }

        self.inner.retain(|v| !v.slice.is_empty());
        #[cfg(feature = "internal_assert")]
        self.assert_normal(&format!(
            "Not sorted after `swap_remove` while removing: {slot}"
        ));

        self.inner
            .iter()
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
}
