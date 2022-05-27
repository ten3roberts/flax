use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Archetype, Changes, EntitySlice},
    ComponentId,
};

pub trait Filter {
    /// Filters a slice of entity slots and returns a subset of the slice
    fn filter(&mut self, slots: EntitySlice) -> EntitySlice;
}

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
}

impl<'a> Filter for ChangeFilter<'a> {
    fn filter(&mut self, slots: EntitySlice) -> EntitySlice {
        loop {
            let cur = match self.cur {
                Some(ref v) => v,
                None => loop {
                    let v = self.changes.get(self.index);
                    if let Some(&(slice, tick)) = v {
                        self.index += 1;
                        if tick > self.tick {
                            break self.cur.get_or_insert(slice);
                        }
                    } else {
                        return EntitySlice::empty();
                    };
                },
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
    fn filter(&mut self, slots: EntitySlice) -> EntitySlice {
        let l = self.left.filter(slots);
        let r = self.right.filter(slots);
        match l.union(&r) {
            Some(v) => v,
            None => {
                // The slices where not contiguous
                // Return the left half for this run.
                // The right will be kept
                l
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
    fn filter(&mut self, slots: EntitySlice) -> EntitySlice {
        let l = self.left.filter(slots);
        let r = self.right.filter(slots);
        l.intersect(&r)
    }
}

#[cfg(test)]
mod tests {
    use atomic_refcell::AtomicRefCell;
    use itertools::Itertools;

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

        let chunks = (0..)
            .scan(slots, |slots, _| {
                let new = filter.filter(*slots);

                if new.is_empty() {
                    return None;
                }

                dbg!(new);
                assert!(new.is_subset(slots));

                let (l, m, r) = slots.split_with(&new).unwrap();

                eprintln!("l: {l:?}, m: {m:?}, r: {r:?}");

                // The left can be skipped.
                // The middle can be chosen and iterated.
                // The right part is undecided and will be determined in the next
                // iteration

                *slots = r;
                Some(m)
            })
            .collect_vec();

        assert_eq!(
            chunks,
            [
                EntitySlice::new(39, 60),
                EntitySlice::new(560, 893),
                EntitySlice::new(945, 1139)
            ]
        );
    }
}
