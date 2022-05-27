use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
};

use itertools::Itertools;

use super::{Slice, Slot};

#[derive(Default, Clone, PartialEq)]
/// A self compacting change tracking which holds either singular changes or a
/// range of changes, automatically merging adjacent ones.
///
///
/// The changes are always stored in a non-overlapping ascending order.
pub struct Changes {
    inner: Vec<(Slice, u32)>,
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

    pub fn as_set(&self, tick: u32) -> BTreeSet<Slot> {
        self.inner
            .iter()
            .filter(|(_, t)| *t > tick)
            .flat_map(|(v, _)| v.iter())
            .collect()
    }

    pub fn as_map(&self) -> BTreeMap<Slot, u32> {
        self.inner
            .iter()
            .flat_map(|&(slice, tick)| slice.iter().map(move |v| (v, tick)))
            .collect()
    }

    pub fn set(&mut self, slots: Slice, change_tick: u32) -> &mut Self {
        let mut insert_point = 0;
        let mut i = 0;
        let mut joined = false;

        eprintln!("Setting: {slots:?}");

        self.inner.retain_mut(|(v, tick)| {
            if *tick < change_tick {
                if let Some(diff) = v.difference(&slots) {
                    eprintln!("Reduced change slice {tick} from {v:?} to {diff:?}");
                    *v = diff;
                } else {
                    eprintln!("No difference of {v:?} and {slots:?}");
                }
            }
            if *tick == change_tick {
                // Merge atop change of the same change
                if let Some(u) = v.union(&slots) {
                    joined = true;
                    *v = u;
                }
            }

            if v.is_empty() {
                false
            } else if v.start < slots.start {
                insert_point += 1;
                true
            } else {
                i += 1;
                true
            }
        });

        if !joined {
            eprintln!("Inserting {insert_point}");
            self.inner.insert(insert_point, (slots, change_tick));
        }

        eprintln!("{:?}", self.inner);

        assert_eq!(
            self.inner
                .iter()
                .copied()
                .sorted_by_key(|v| v.0.start)
                .collect_vec(),
            self.inner
        );

        self

        // match self.inner.last_mut() {
        //     Some((v, tick)) if *tick == change_tick => {
        //         eprintln!("Attempting to unionize");
        //         if let Some(u) = v.union(&slice) {
        //             eprintln!("Union");
        //             *v = u
        //         } else {
        //             eprintln!("No union, pushing new");
        //             self.inner.push((slice, change_tick))
        //         }
        //     }
        //     _ => {
        //         eprintln!("Pushing new change for {change_tick}");
        //         self.inner.push((slice, change_tick))
        //     }
        // }
    }

    /// Returns the changes in the change list at a particular index.
    pub fn get(&self, index: usize) -> Option<&(Slice, u32)> {
        self.inner.get(index)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Iterate all changes in ascending order
    pub fn iter(&self) -> std::slice::Iter<(Slice, u32)> {
        self.inner.iter()
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;
    #[test]
    fn changes() {
        let mut changes = Changes::new();

        changes.set(Slice::new(0, 5), 1);

        changes.set(Slice::new(70, 92), 2);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [(Slice::new(0, 5), 1), (Slice::new(70, 92), 2)]
        );

        changes.set(Slice::new(3, 5), 3);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                (Slice::new(0, 3), 1),
                (Slice::new(3, 5), 3),
                (Slice::new(70, 92), 2),
            ]
        );

        // Extend previous change
        changes.set(Slice::new(4, 14), 3);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                (Slice::new(0, 3), 1),
                (Slice::new(3, 14), 3),
                (Slice::new(70, 92), 2),
            ]
        );

        // Overwrite almost all
        changes.set(Slice::new(0, 89), 4);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [(Slice::new(0, 89), 4), (Slice::new(89, 92), 2),]
        );
    }

    #[test]
    fn changes_small() {
        let mut changes = Changes::new();

        for i in 0..239 {
            let perm = (i * (i + 2)) % 300;
            // let perm = i;
            changes.set(Slice::new(perm, perm), i as _);
        }

        changes.set(Slice::new(70, 249), 300);
        changes.set(Slice::new(0, 89), 301);
        changes.set(Slice::new(209, 300), 302);

        eprintln!("Changes: {changes:#?}");
    }

    #[test]
    fn adjacent() {
        let mut changes = Changes::new();

        changes.set(Slice::new(0, 63), 1);
        changes.set(Slice::new(63, 182), 1);
        assert_eq!(
            changes.iter().copied().collect_vec(),
            [(Slice::new(0, 182), 1)]
        );
    }
}
