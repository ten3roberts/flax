use std::iter::FusedIterator;

use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Archetype, Changes, EntitySlice},
    ComponentId,
};

pub trait Filter {
    /// Filters a slice of entity slots and returns a subset of the slice
    fn filter(&mut self, slots: EntitySlice) -> Option<EntitySlice>;
}

#[derive(Debug)]
pub struct ChangeFilter<'a> {
    changes: AtomicRef<'a, Changes>,
    cur: Option<EntitySlice>,
    // The current change group.
    // Starts at the end and decrements
    index: usize,
    tick: u32,
}

impl<'a> ChangeFilter<'a> {
    pub fn new(archetype: &'a Archetype, component: ComponentId, tick: u32) -> Self {
        let changes = archetype.changes(component).unwrap();
        Self::from_borrow(changes, tick)
    }

    pub(crate) fn from_borrow(changes: AtomicRef<'a, Changes>, tick: u32) -> Self {
        Self {
            changes,
            cur: None,
            index: 0,
            tick,
        }
    }

    pub fn current_slice(&mut self) -> Option<&EntitySlice> {
        match self.cur {
            Some(ref v) => Some(v),
            None => loop {
                let v = self.changes.get(self.index);
                if let Some(&(slice, tick)) = v {
                    self.index += 1;
                    if tick > self.tick {
                        break Some(self.cur.get_or_insert(slice));
                    }
                } else {
                    // No more
                    return None;
                };
            },
        }
    }
}

impl<'a> Filter for ChangeFilter<'a> {
    fn filter(&mut self, slots: EntitySlice) -> Option<EntitySlice> {
        loop {
            let cur = self.current_slice()?;

            let intersect = cur.intersect(&slots);
            // Try again with the next change group
            if intersect.is_empty() {
                self.cur = None;
                continue;
            } else {
                return Some(intersect);
            }
        }
    }
}

/// Or filter combinator
pub struct Or<L, R> {
    left: L,
    right: R,
}

impl<L, R> Or<L, R> {
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<L, R> Filter for Or<L, R>
where
    L: Filter,
    R: Filter,
{
    fn filter(&mut self, slots: EntitySlice) -> Option<EntitySlice> {
        let l = self.left.filter(slots)?;
        let r = self.right.filter(slots)?;
        match l.union(&r) {
            Some(v) => Some(v),
            None => {
                // The slices where not contiguous
                // Return the left half for this run.
                // The right will be kept
                Some(l)
            }
        }
    }
}

/// And filter combinator
pub struct And<L, R> {
    left: L,
    right: R,
}

impl<L, R> And<L, R> {
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<L, R> Filter for And<L, R>
where
    L: Filter,
    R: Filter,
{
    fn filter(&mut self, slots: EntitySlice) -> Option<EntitySlice> {
        let l = self.left.filter(slots)?;
        let r = self.right.filter(slots)?;

        let i = l.intersect(&r);
        if i.is_empty() {
            // Go again but start with the highest bound
            // This is caused by one of the sides being past the end of the
            // other slice. As such, force the slice lagging behind to catch up
            // to the upper floor
            let max = l.start.max(r.start).min(slots.end);

            eprintln!("Retrying with {max}");
            let slots = EntitySlice::new(max, slots.end);
            let l = self.left.filter(slots)?;
            let r = self.right.filter(slots)?;
            Some(l.intersect(&r))
        } else {
            eprintln!("{l:?} && {r:?} => {i:?}");
            Some(i)
        }
    }
}

pub struct FilterIter<F> {
    slots: EntitySlice,
    filter: F,
}

impl<F> FilterIter<F> {
    pub fn new(slots: EntitySlice, filter: F) -> Self {
        Self { slots, filter }
    }
}

impl<F: Filter> Iterator for FilterIter<F> {
    type Item = EntitySlice;

    fn next(&mut self) -> Option<Self::Item> {
        let cur = self.filter.filter(self.slots)?;

        if cur.is_empty() {
            None
        } else {
            let (_l, m, r) = self
                .slots
                .split_with(&cur)
                .expect("Return value of filter must be a subset of `slots");

            self.slots = r;
            Some(m)
        }
    }
}

impl<F: Filter> FusedIterator for FilterIter<F> {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use atomic_refcell::AtomicRefCell;
    use itertools::Itertools;

    use crate::archetype::Slot;

    use super::*;
    #[test]
    fn filter() {
        let mut changes = Changes::new();

        changes.set(EntitySlice::new(40, 200), 1);
        changes.set(EntitySlice::new(70, 349), 2);

        changes.set(EntitySlice::new(560, 893), 5);
        changes.set(EntitySlice::new(39, 60), 6);
        changes.set(EntitySlice::new(784, 800), 7);
        changes.set(EntitySlice::new(945, 1139), 8);

        dbg!(&changes);

        let changes = AtomicRefCell::new(changes);

        let mut filter = ChangeFilter::from_borrow(changes.borrow(), 2);

        // The whole "archetype"
        let slots = EntitySlice::new(0, 1238);

        // let chunks = (0..)
        //     .scan(slots, |slots, _| {
        //         let new = filter.filter(*slots);

        //         if new.is_empty() {
        //             return None;
        //         }

        //         dbg!(new);
        //         assert!(new.is_subset(slots));

        //         let (l, m, r) = slots.split_with(&new).unwrap();

        //         eprintln!("l: {l:?}, m: {m:?}, r: {r:?}");

        //         // The left can be skipped.
        //         // The middle can be chosen and iterated.
        //         // The right part is undecided and will be determined in the next
        //         // iteration

        //         *slots = r;
        //         Some(m)
        //     })
        //     .collect_vec();

        let chunks = FilterIter::new(slots, filter).collect_vec();

        assert_eq!(
            chunks,
            [
                EntitySlice::new(39, 60),
                EntitySlice::new(560, 893),
                EntitySlice::new(945, 1139)
            ]
        );
    }

    #[test]
    fn combinators() {
        let mut changes_1 = Changes::new();
        let mut changes_2 = Changes::new();

        changes_1.set(EntitySlice::new(40, 65), 2);
        changes_1.set(EntitySlice::new(59, 80), 3);
        changes_1.set(EntitySlice::new(90, 234), 3);

        changes_2.set(EntitySlice::new(50, 70), 3);
        changes_2.set(EntitySlice::new(99, 210), 4);

        let a_map = changes_1.as_map();
        let b_map = changes_2.as_map();

        eprintln!("Changes: \n  {changes_1:?}\n  {changes_2:?}");
        let changes_1 = AtomicRefCell::new(changes_1);
        let changes_2 = AtomicRefCell::new(changes_2);

        let slots = EntitySlice::new(0, 1000);

        // Or
        let a = ChangeFilter::from_borrow(changes_1.borrow(), 1);
        let b = ChangeFilter::from_borrow(changes_2.borrow(), 2);

        let filter = Or::new(a, b);

        // Use a brute force BTreeSet for solving it
        let chunks_set = slots
            .iter()
            .filter(|v| *a_map.get(v).unwrap_or(&0) > 1 || *b_map.get(v).unwrap_or(&0) > 2)
            .collect_vec();

        let chunks = FilterIter::new(slots, filter).flatten().collect_vec();

        assert_eq!(chunks, chunks_set);

        // And

        let a = ChangeFilter::from_borrow(changes_1.borrow(), 1);
        let b = ChangeFilter::from_borrow(changes_2.borrow(), 2);
        let filter = And::new(a, b);

        // Use a brute force BTreeSet for solving it
        let chunks_set = slots
            .iter()
            .filter(|v| *a_map.get(v).unwrap_or(&0) > 1 && *b_map.get(v).unwrap_or(&0) > 2)
            .collect_vec();

        let chunks = FilterIter::new(slots, filter).flatten().collect_vec();

        assert_eq!(chunks, chunks_set,);
    }
}
