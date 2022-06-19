mod cmp;

use std::{iter::FusedIterator, ops::Neg};

use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Archetype, ChangeKind, Changes, Slice},
    ComponentId,
};

pub use cmp::CmpExt;

macro_rules! gen_bitops {
    ($ty:ident[$($p: tt),*]) => {
        impl<R, $($p),*> std::ops::BitOr<R> for $ty<$($p),*>
        where
            Self: for<'x, 'y> Filter<'x, 'y>,
            R: for<'x,'y> Filter<'x, 'y>,
        {
            type Output = Or<Self, R>;

            fn bitor(self, rhs: R) -> Self::Output {
                self.or(rhs)
            }
        }

        impl<'a, R, $($p),*> std::ops::BitAnd<R> for $ty<$($p),*>
        where
            Self: for<'x, 'y> Filter<'x, 'y>,
            R: for<'x,'y> Filter<'x, 'y>,
        {
            type Output = And<Self, R>;

            fn bitand(self, rhs: R) -> Self::Output {
                self.and(rhs)
            }
        }

        impl<$($p),*> std::ops::Neg for $ty<$($p),*>
        where
            Self: for<'x, 'y> Filter<'x, 'y>
        {
            type Output = Not<Self>;

            fn neg(self) -> Self::Output {
                Not(self)
            }
        }
    };


    ($($ty:ident[$($p: tt),*];)*) => {
        $(
            gen_bitops!{ $ty[$($p),*] }
        )*
    }
}

gen_bitops! {
    ModifiedFilter[];
    InsertedFilter[];
    RemovedFilter[];
    And[A,B];
    Or[A,B];
    All[];
    Nothing[];
    With[];
    Without[];
}

/// A filter over a query which will be prepared for an archetype, yielding
/// subsets of slots.
pub trait Filter<'this, 'w>
where
    Self: Sized,
{
    type Prepared: PreparedFilter;

    /// Prepare the filter for an archetype.
    /// `change_tick` refers to the last time this query was run. Useful for
    /// change detection.
    fn prepare(&'this self, archetype: &'w Archetype, change_tick: u32) -> Self::Prepared;

    fn or<F: for<'x> Filter<'x, 'w>>(self, other: F) -> Or<Self, F> {
        Or {
            left: self,
            right: other,
        }
    }

    fn and<F: for<'x> Filter<'x, 'w>>(self, other: F) -> And<Self, F> {
        And {
            left: self,
            right: other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModifiedFilter {
    component: ComponentId,
}

impl ModifiedFilter {
    pub fn new(component: ComponentId) -> Self {
        Self { component }
    }
}

impl<'this, 'a> Filter<'this, 'a> for ModifiedFilter {
    type Prepared = PreparedKindFilter<'a, fn(&ChangeKind) -> bool>;

    fn prepare(&self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedKindFilter::new(
            archetype,
            self.component,
            change_tick,
            ChangeKind::is_modified_or_inserted,
        )
    }
}

#[derive(Debug, Clone)]
pub struct InsertedFilter {
    component: ComponentId,
}

impl InsertedFilter {
    pub fn new(component: ComponentId) -> Self {
        Self { component }
    }
}

impl<'this, 'a> Filter<'this, 'a> for InsertedFilter {
    type Prepared = PreparedKindFilter<'a, fn(&ChangeKind) -> bool>;

    fn prepare(&self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedKindFilter::new(
            archetype,
            self.component,
            change_tick,
            ChangeKind::is_inserted,
        )
    }
}

#[derive(Debug, Clone)]
pub struct RemovedFilter {
    component: ComponentId,
}

impl RemovedFilter {
    pub fn new(component: ComponentId) -> Self {
        Self { component }
    }
}

impl<'this, 'a> Filter<'this, 'a> for RemovedFilter {
    type Prepared = PreparedKindFilter<'a, fn(&ChangeKind) -> bool>;

    fn prepare(&self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedKindFilter::new(
            archetype,
            self.component,
            change_tick,
            ChangeKind::is_removed,
        )
    }
}

#[derive(Debug, Clone)]
pub struct And<L, R> {
    left: L,
    right: R,
}

impl<L, R> And<L, R> {
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<'this, 'a, L, R> Filter<'this, 'a> for And<L, R>
where
    L: Filter<'this, 'a>,
    R: Filter<'this, 'a>,
{
    type Prepared = PreparedAnd<L::Prepared, R::Prepared>;

    fn prepare(&'this self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedAnd {
            left: self.left.prepare(archetype, change_tick),
            right: self.right.prepare(archetype, change_tick),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Or<L, R> {
    left: L,
    right: R,
}

impl<L, R> Or<L, R> {
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<'this, 'a, L, R> Filter<'this, 'a> for Or<L, R>
where
    L: Filter<'this, 'a>,
    R: Filter<'this, 'a>,
{
    type Prepared = PreparedOr<L::Prepared, R::Prepared>;

    fn prepare(&'this self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedOr {
            left: self.left.prepare(archetype, change_tick),
            right: self.right.prepare(archetype, change_tick),
        }
    }
}

pub trait PreparedFilter {
    /// Filters a slice of entity slots and returns a subset of the slice
    fn filter(&mut self, slots: Slice) -> Slice;
}

#[derive(Debug)]
pub struct PreparedKindFilter<'a, F> {
    changes: Option<AtomicRef<'a, Changes>>,
    cur: Option<Slice>,
    // The current change group.
    // Starts at the end and decrements
    index: usize,
    tick: u32,
    filter: F,
}

impl<'a, F> PreparedKindFilter<'a, F>
where
    F: Fn(&ChangeKind) -> bool,
{
    pub fn new(archetype: &'a Archetype, component: ComponentId, tick: u32, filter: F) -> Self {
        let changes = archetype.changes(component);
        Self {
            changes,
            cur: None,
            index: 0,
            tick,
            filter,
        }
    }

    #[cfg(test)]
    fn from_borrow(changes: AtomicRef<'a, Changes>, tick: u32, filter: F) -> Self {
        Self {
            changes: Some(changes),
            cur: None,
            index: 0,
            tick,
            filter,
        }
    }

    pub fn current_slice(&mut self) -> Option<Slice> {
        match (self.cur, self.changes.as_mut()) {
            (Some(v), _) => Some(v),
            (None, Some(changes)) => loop {
                let v = changes.get(self.index);
                if let Some(change) = v {
                    self.index += 1;
                    if change.tick > self.tick && (self.filter)(&change.kind) {
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

impl<'a, F> PreparedFilter for PreparedKindFilter<'a, F>
where
    F: Fn(&ChangeKind) -> bool,
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
}

/// Or filter combinator
pub struct PreparedOr<L, R> {
    left: L,
    right: R,
}

impl<L, R> PreparedOr<L, R> {
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<L, R> PreparedFilter for PreparedOr<L, R>
where
    L: PreparedFilter,
    R: PreparedFilter,
{
    fn filter(&mut self, slots: Slice) -> Slice {
        let l = self.left.filter(slots);
        let r = self.right.filter(slots);
        let u = l.union(&r);
        eprintln!("l: {l:?} r: {r:?} u: {u:?}");
        match u {
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

#[derive(Debug, Clone)]
pub struct Not<T>(pub T);

impl<'this, 'a, T> Filter<'this, 'a> for Not<T>
where
    T: Filter<'this, 'a>,
{
    type Prepared = PreparedNot<T::Prepared>;

    fn prepare(&'this self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedNot(self.0.prepare(archetype, change_tick))
    }
}

impl<R, T> std::ops::BitOr<R> for Not<T>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    R: for<'x, 'y> Filter<'x, 'y>,
{
    type Output = Or<Self, R>;

    fn bitor(self, rhs: R) -> Self::Output {
        Or::new(self, rhs)
    }
}

impl<R, T> std::ops::BitAnd<R> for Not<T>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    R: for<'x, 'y> Filter<'x, 'y>,
{
    type Output = And<Self, R>;

    fn bitand(self, rhs: R) -> Self::Output {
        And::new(self, rhs)
    }
}

impl<'a, T> Neg for Not<T>
where
    T: for<'x, 'y> Filter<'x, 'y>,
{
    type Output = T;

    fn neg(self) -> Self::Output {
        self.0
    }
}

pub struct PreparedNot<T>(T);

impl<T> PreparedFilter for PreparedNot<T>
where
    T: PreparedFilter,
{
    fn filter(&mut self, slots: Slice) -> Slice {
        let a = self.0.filter(slots);

        slots.difference(&a).unwrap()
    }
}

/// And filter combinator
pub struct PreparedAnd<L, R> {
    left: L,
    right: R,
}

impl<L, R> PreparedAnd<L, R> {
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<L, R> PreparedFilter for PreparedAnd<L, R>
where
    L: PreparedFilter,
    R: PreparedFilter,
{
    fn filter(&mut self, slots: Slice) -> Slice {
        let l = self.left.filter(slots);
        let r = self.right.filter(slots);

        let i = l.intersect(&r);
        if i.is_empty() {
            // Go again but start with the highest bound
            // This is caused by one of the sides being past the end of the
            // other slice. As such, force the slice lagging behind to catch up
            // to the upper floor
            let max = l.start.max(r.start).min(slots.end);

            let slots = Slice::new(max, slots.end);
            let l = self.left.filter(slots);
            let r = self.right.filter(slots);
            l.intersect(&r)
        } else {
            i
        }
    }
}

#[derive(Debug, Clone)]
pub struct Nothing;

impl<'this, 'a> Filter<'this, 'a> for Nothing {
    type Prepared = BooleanFilter;

    fn prepare(&'this self, _: &'a Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(false)
    }
}

#[derive(Debug, Clone)]
pub struct All;

impl<'this, 'a> Filter<'this, 'a> for All {
    type Prepared = BooleanFilter;

    fn prepare(&self, _: &'a Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(true)
    }
}

#[derive(Debug, Clone)]
pub struct FilterIter<F> {
    slots: Slice,
    filter: F,
}

impl<F> FilterIter<F> {
    pub fn new(slots: Slice, filter: F) -> Self {
        Self { slots, filter }
    }
}

impl<F> Iterator for FilterIter<F>
where
    F: PreparedFilter,
{
    type Item = Slice;

    fn next(&mut self) -> Option<Self::Item> {
        eprintln!("Here");
        let cur = self.filter.filter(self.slots);

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

impl<F: PreparedFilter> FusedIterator for FilterIter<F> {}

pub struct With {
    component: ComponentId,
}

impl With {
    pub fn new(component: ComponentId) -> Self {
        Self { component }
    }
}

impl<'this, 'a> Filter<'this, 'a> for With {
    type Prepared = BooleanFilter;

    fn prepare(&self, archetype: &'a Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(archetype.has(self.component))
    }
}

pub struct Without {
    component: ComponentId,
}

impl Without {
    pub fn new(component: ComponentId) -> Self {
        Self { component }
    }
}

impl<'this, 'a> Filter<'this, 'a> for Without {
    type Prepared = BooleanFilter;

    fn prepare(&self, archetype: &'a Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(!archetype.has(self.component))
    }
}

pub struct BooleanFilter(bool);

impl PreparedFilter for BooleanFilter {
    fn filter(&mut self, slots: Slice) -> Slice {
        if self.0 {
            slots
        } else {
            Slice::empty()
        }
    }
}

#[cfg(test)]
mod tests {

    use atomic_refcell::AtomicRefCell;
    use itertools::Itertools;

    use crate::archetype::Change;

    use super::*;
    #[test]
    fn filter() {
        let mut changes = Changes::new();

        changes.set(Change::modified(Slice::new(40, 200), 1));
        changes.set(Change::modified(Slice::new(70, 349), 2));
        changes.set(Change::modified(Slice::new(560, 893), 5));
        changes.set(Change::modified(Slice::new(39, 60), 6));
        changes.set(Change::inserted(Slice::new(784, 800), 7));
        changes.set(Change::modified(Slice::new(945, 1139), 8));

        let changes = AtomicRefCell::new(changes);

        let filter = PreparedKindFilter::from_borrow(
            changes.borrow(),
            2,
            ChangeKind::is_modified_or_inserted,
        );

        // The whole "archetype"
        let slots = Slice::new(0, 1238);

        let chunks = FilterIter::new(slots, filter).collect_vec();

        assert_eq!(
            chunks,
            [
                Slice::new(39, 60),
                Slice::new(560, 893),
                Slice::new(945, 1139)
            ]
        );
    }

    #[test]
    fn combinators() {
        let mut changes_1 = Changes::new();
        let mut changes_2 = Changes::new();

        changes_1.set(Change::modified(Slice::new(40, 65), 2));
        changes_1.set(Change::modified(Slice::new(59, 80), 3));
        changes_1.set(Change::modified(Slice::new(90, 234), 3));
        changes_2.set(Change::modified(Slice::new(50, 70), 3));
        changes_2.set(Change::modified(Slice::new(99, 210), 4));

        let a_map = changes_1.as_changed_set(1);
        let b_map = changes_2.as_changed_set(2);

        eprintln!("Changes: \n  {changes_1:?}\n  {changes_2:?}");
        let changes_1 = AtomicRefCell::new(changes_1);
        let changes_2 = AtomicRefCell::new(changes_2);

        let slots = Slice::new(0, 1000);

        // Or
        let a = PreparedKindFilter::from_borrow(
            changes_1.borrow(),
            1,
            ChangeKind::is_modified_or_inserted,
        );
        let b = PreparedKindFilter::from_borrow(
            changes_2.borrow(),
            2,
            ChangeKind::is_modified_or_inserted,
        );

        let filter = PreparedOr::new(a, b);

        // Use a brute force BTreeSet for solving it
        let chunks_set = slots
            .iter()
            .filter(|v| a_map.contains(v) || b_map.contains(v))
            .collect_vec();

        let chunks = FilterIter::new(slots, filter).flatten().collect_vec();

        assert_eq!(chunks, chunks_set);

        // And

        let a = PreparedKindFilter::from_borrow(
            changes_1.borrow(),
            1,
            ChangeKind::is_modified_or_inserted,
        );
        let b = PreparedKindFilter::from_borrow(
            changes_2.borrow(),
            2,
            ChangeKind::is_modified_or_inserted,
        );
        let filter = PreparedAnd::new(a, b);

        // Use a brute force BTreeSet for solving it
        let chunks_set = slots
            .iter()
            .filter(|v| a_map.contains(v) && b_map.contains(v))
            .collect_vec();

        let chunks = FilterIter::new(slots, filter).flatten().collect_vec();

        assert_eq!(chunks, chunks_set,);
    }

    #[test]
    fn archetypes() {
        component! {
            a: i32,
            b: String,
            c: u32,
        }

        let archetype = Archetype::new([a().info(), b().info(), c().info()]);

        let filter = ModifiedFilter::new(a().id()) & (ModifiedFilter::new(b().id()))
            | (ModifiedFilter::new(c().id()));

        // Mock changes
        let a_map = archetype
            .changes_mut(a().id())
            .unwrap()
            .set(Change::modified(Slice::new(9, 80), 2))
            .set(Change::modified(Slice::new(65, 83), 4))
            .as_changed_set(1);

        let b_map = archetype
            .changes_mut(b().id())
            .unwrap()
            .set(Change::modified(Slice::new(16, 45), 2))
            .set(Change::modified(Slice::new(68, 85), 2))
            .as_changed_set(1);

        let c_map = archetype
            .changes_mut(c().id())
            .unwrap()
            .set(Change::modified(Slice::new(96, 123), 3))
            .as_changed_set(1);

        // Brute force

        let slots = Slice::new(0, 1000);
        let chunks_set = slots
            .iter()
            .filter(|v| (a_map.contains(v) && b_map.contains(v)) || (c_map.contains(v)))
            .collect_vec();

        let chunks = FilterIter::new(slots, filter.prepare(&archetype, 1))
            .inspect(|v| eprintln!("Changes: {v:?}"))
            .flatten()
            .collect_vec();

        // assert_eq!(chunks, chunks_set);
    }
}
