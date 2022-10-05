mod change;
mod cmp;
use alloc::vec::Vec;

use core::{
    any::type_name,
    iter::FusedIterator,
    ops::{self, Neg},
};

use crate::{
    archetype::{Archetype, Slice, Slot},
    Access, ArchetypeId, ComponentKey, ComponentValue, Entity,
};

pub use change::*;
pub use cmp::CmpExt;

macro_rules! gen_bitops {
    ($ty:ident[$($p: tt),*]) => {
        impl<R, $($p: ComponentValue),*> ops::BitOr<R> for $ty<$($p),*>
        {
            type Output = Or<Self, R>;

            fn bitor(self, rhs: R) -> Self::Output {
                Or::new(self, rhs)
            }
        }

        impl<R, $($p: ComponentValue),*> ops::BitAnd<R> for $ty<$($p),*>
        {
            type Output = And<Self, R>;

            fn bitand(self, rhs: R) -> Self::Output {
                And::new(self, rhs)
            }
        }

        impl<$($p: ComponentValue),*> ops::Neg for $ty<$($p),*>
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
    ChangeFilter[T];
    RemovedFilter[T];
    And[A,B];
    Or[A,B];
    All[];
    Nothing[];
    With[];
    Without[];
}

/// A filter which does not depend upon any state, such as a `with` filter
pub trait StaticFilter {
    /// Returns true if the filter matches the archetype without state
    fn static_matches(&self, arch: &Archetype) -> bool;
}

/// A filter over a query which will be prepared for an archetype, yielding
/// subsets of slots.
///
/// A filter requires Debug for error messages for user conveniance
pub trait Filter<'w>
where
    Self: Sized + core::fmt::Debug,
{
    /// The filter holding possible borrows
    type Prepared: PreparedFilter + 'w;

    /// Prepare the filter for an archetype.
    /// `change_tick` refers to the last time this query was run. Useful for
    /// change detection.
    fn prepare(&'w self, arch: &'w Archetype, change_tick: u32) -> Self::Prepared;

    /// Returns true if the filter will yield at least one entity from the
    /// archetype.
    ///
    /// Returns false if an entity will never yield, such as a mismatched
    /// archetype
    fn matches(&self, arch: &Archetype) -> bool;
    /// Returns which components and how will be accessed for an archetype.
    fn access(&self, id: ArchetypeId, arch: &Archetype) -> Vec<Access>;
}

/// The prepared version of a filter, which can hold borrows from the world
pub trait PreparedFilter {
    /// Filters a slice of entity slots and returns a subset of the slice
    fn filter(&mut self, slots: Slice) -> Slice;
}

#[derive(Debug, Clone)]
/// And filter combinator
pub struct And<L, R> {
    left: L,
    right: R,
}

impl<L, R> And<L, R> {
    /// Creates a new and filter
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<'a, L, R> Filter<'a> for And<L, R>
where
    L: Filter<'a>,
    R: Filter<'a>,
{
    type Prepared = PreparedAnd<L::Prepared, R::Prepared>;

    fn prepare(&'a self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedAnd {
            left: self.left.prepare(archetype, change_tick),
            right: self.right.prepare(archetype, change_tick),
        }
    }

    fn matches(&self, archetype: &Archetype) -> bool {
        self.left.matches(archetype) && self.right.matches(archetype)
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        let mut res = self.left.access(id, archetype);
        res.append(&mut self.right.access(id, archetype));
        res
    }
}

impl<L, R> StaticFilter for And<L, R>
where
    L: StaticFilter,
    R: StaticFilter,
{
    fn static_matches(&self, archetype: &Archetype) -> bool {
        self.left.static_matches(archetype) && self.right.static_matches(archetype)
    }
}

#[derive(Debug, Clone)]
/// Or filter combinator
pub struct Or<L, R> {
    left: L,
    right: R,
}

impl<L, R> Or<L, R> {
    /// Creates a new or filter
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<'a, L, R> Filter<'a> for Or<L, R>
where
    L: Filter<'a>,
    R: Filter<'a>,
{
    type Prepared = PreparedOr<L::Prepared, R::Prepared>;

    fn prepare(&'a self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedOr {
            left: self.left.prepare(archetype, change_tick),
            right: self.right.prepare(archetype, change_tick),
        }
    }

    fn matches(&self, archetype: &Archetype) -> bool {
        self.left.matches(archetype) || self.right.matches(archetype)
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        let mut accesses = self.left.access(id, archetype);
        accesses.append(&mut self.right.access(id, archetype));
        accesses
    }
}

impl<L, R> StaticFilter for Or<L, R>
where
    L: StaticFilter,
    R: StaticFilter,
{
    fn static_matches(&self, archetype: &Archetype) -> bool {
        self.left.static_matches(archetype) || self.right.static_matches(archetype)
    }
}

/// Or filter combinator
#[doc(hidden)]
pub struct PreparedOr<L, R> {
    left: L,
    right: R,
}

impl<L, R> PreparedFilter for PreparedOr<L, R>
where
    L: PreparedFilter,
    R: PreparedFilter,
{
    #[inline(always)]
    fn filter(&mut self, slots: Slice) -> Slice {
        let l = self.left.filter(slots);
        let r = self.right.filter(slots);
        let u = l.union(&r);
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
/// Negate a filter
pub struct Not<T>(pub T);

impl<'a, T> Filter<'a> for Not<T>
where
    T: Filter<'a>,
{
    type Prepared = PreparedNot<T::Prepared>;

    fn prepare(&'a self, archetype: &'a Archetype, change_tick: u32) -> Self::Prepared {
        PreparedNot(self.0.prepare(archetype, change_tick))
    }

    fn matches(&self, archetype: &Archetype) -> bool {
        !self.0.matches(archetype)
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        self.0.access(id, archetype)
    }
}

impl<T> StaticFilter for Not<T>
where
    T: StaticFilter,
{
    fn static_matches(&self, archetype: &Archetype) -> bool {
        !self.0.static_matches(archetype)
    }
}

impl<R, T> ops::BitOr<R> for Not<T>
where
    Self: for<'x> Filter<'x>,
    R: for<'x> Filter<'x>,
{
    type Output = Or<Self, R>;

    fn bitor(self, rhs: R) -> Self::Output {
        Or::new(self, rhs)
    }
}

impl<R, T> ops::BitAnd<R> for Not<T>
where
    Self: for<'x> Filter<'x>,
    R: for<'x> Filter<'x>,
{
    type Output = And<Self, R>;

    fn bitand(self, rhs: R) -> Self::Output {
        And::new(self, rhs)
    }
}

impl<T> Neg for Not<T>
where
    T: for<'x> Filter<'x>,
{
    type Output = T;

    fn neg(self) -> Self::Output {
        self.0
    }
}

#[doc(hidden)]
pub struct PreparedNot<T>(T);

impl<T> PreparedFilter for PreparedNot<T>
where
    T: PreparedFilter,
{
    fn filter(&mut self, slots: Slice) -> Slice {
        let a = self.0.filter(slots);

        slots.difference(a).unwrap()
    }
}

/// And filter combinator
#[doc(hidden)]
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
    #[inline(always)]
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
/// A filter that yields, well, nothing
pub struct Nothing;

impl<'a> Filter<'a> for Nothing {
    type Prepared = BooleanFilter;

    #[inline(always)]
    fn prepare(&self, _: &'a Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(false)
    }

    #[inline(always)]
    fn matches(&self, _: &Archetype) -> bool {
        false
    }

    #[inline(always)]
    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

impl StaticFilter for Nothing {
    fn static_matches(&self, _: &Archetype) -> bool {
        false
    }
}

/// Yields all entities
#[derive(Debug, Clone)]
pub struct All;

impl<'a> Filter<'a> for All {
    type Prepared = BooleanFilter;

    #[inline(always)]
    fn prepare(&self, _: &Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(true)
    }

    #[inline(always)]
    fn matches(&self, _: &Archetype) -> bool {
        true
    }

    #[inline(always)]
    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

impl StaticFilter for All {
    fn static_matches(&self, _: &Archetype) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
/// Iterator which yields slices which match the underlying filter
pub struct FilterIter<F> {
    slots: Slice,
    filter: F,
}

impl<F> FilterIter<F> {
    /// Creates a new filter iterator visiting the `slot` of the same archetype
    /// as `F`
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

#[derive(Debug, Clone)]
/// Filter which only yields true if the entity has the specified component
pub struct With {
    pub(crate) component: ComponentKey,
}

impl StaticFilter for With {
    fn static_matches(&self, arch: &Archetype) -> bool {
        arch.has(self.component)
    }
}

impl<'a> Filter<'a> for With {
    type Prepared = BooleanFilter;

    fn prepare(&self, arch: &Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(self.matches(arch))
    }

    fn matches(&self, arch: &Archetype) -> bool {
        arch.has(self.component)
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

#[derive(Debug, Clone)]
/// Opposite of [crate::Without]
pub struct Without {
    pub(crate) component: ComponentKey,
}

impl<'a> Filter<'a> for Without {
    type Prepared = BooleanFilter;

    fn prepare(&self, arch: &Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(self.matches(arch))
    }

    fn matches(&self, arch: &Archetype) -> bool {
        self.static_matches(arch)
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

impl StaticFilter for Without {
    fn static_matches(&self, arch: &Archetype) -> bool {
        !arch.has(self.component)
    }
}
#[derive(Debug, Clone)]
/// Yields all entitiens with the relation of the specified kind
pub(crate) struct WithObject {
    pub(crate) object: Entity,
}

impl StaticFilter for WithObject {
    fn static_matches(&self, arch: &Archetype) -> bool {
        arch.components().any(|v| {
            if let Some(v) = v.id().object {
                if v == self.object {
                    return true;
                }
            }

            false
        })
    }
}

impl<'a> Filter<'a> for WithObject {
    type Prepared = BooleanFilter;

    fn prepare(&self, arch: &Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(self.matches(arch))
    }

    fn matches(&self, arch: &Archetype) -> bool {
        self.static_matches(arch)
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

pub(crate) struct ArchetypeFilter<F>(pub(crate) F);

impl<F> core::fmt::Debug for ArchetypeFilter<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("ArchetypeFilter")
            .field(&type_name::<F>())
            .finish()
    }
}

impl<F: Fn(&Archetype) -> bool> StaticFilter for ArchetypeFilter<F> {
    fn static_matches(&self, arch: &Archetype) -> bool {
        (self.0)(arch)
    }
}

impl<'w, F: Fn(&Archetype) -> bool> Filter<'w> for ArchetypeFilter<F> {
    type Prepared = BooleanFilter;

    fn prepare(&'w self, arch: &'w Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(self.matches(arch))
    }

    fn matches(&self, arch: &Archetype) -> bool {
        self.static_matches(arch)
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

#[derive(Debug, Clone)]
/// Yields all entitiens with the relation of the specified kind
pub struct WithRelation {
    pub(crate) relation: Entity,
}

impl StaticFilter for WithRelation {
    fn static_matches(&self, arch: &Archetype) -> bool {
        arch.relations_like(self.relation).next().is_some()
    }
}

impl<'a> Filter<'a> for WithRelation {
    type Prepared = BooleanFilter;

    fn prepare(&self, arch: &Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(self.matches(arch))
    }

    fn matches(&self, arch: &Archetype) -> bool {
        self.static_matches(arch)
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

#[derive(Debug, Clone)]
/// Opposite of [crate::Without]
pub struct WithoutRelation {
    pub(crate) relation: Entity,
}

impl<'a> Filter<'a> for WithoutRelation {
    type Prepared = BooleanFilter;

    fn prepare(&self, arch: &Archetype, _: u32) -> Self::Prepared {
        BooleanFilter(self.matches(arch))
    }

    fn matches(&self, arch: &Archetype) -> bool {
        self.static_matches(arch)
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

impl StaticFilter for WithoutRelation {
    fn static_matches(&self, arch: &Archetype) -> bool {
        arch.relations_like(self.relation).next().is_none()
    }
}

/// Like a bool literal
#[derive(Copy, Debug, Clone)]
pub struct BooleanFilter(pub bool);
impl<'w> Filter<'w> for BooleanFilter {
    type Prepared = Self;

    #[inline(always)]
    fn prepare(&'w self, _: &'w Archetype, _: u32) -> Self::Prepared {
        *self
    }

    #[inline(always)]
    fn matches(&self, _: &Archetype) -> bool {
        self.0
    }

    #[inline(always)]
    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

impl PreparedFilter for BooleanFilter {
    #[inline(always)]
    fn filter(&mut self, slots: Slice) -> Slice {
        if self.0 {
            slots
        } else {
            Slice::empty()
        }
    }
}

impl<'w, F> Filter<'w> for &F
where
    F: Filter<'w>,
{
    type Prepared = F::Prepared;

    fn prepare(&'w self, archetype: &'w Archetype, change_tick: u32) -> Self::Prepared {
        (*self).prepare(archetype, change_tick)
    }

    fn matches(&self, arch: &Archetype) -> bool {
        (*self).matches(arch)
    }

    fn access(&self, id: ArchetypeId, arch: &Archetype) -> Vec<Access> {
        (*self).access(id, arch)
    }
}

/// A filter which can be turned on or off
/// When disabled, returns All
#[derive(Debug, Clone)]
pub struct GatedFilter<F> {
    pub(crate) active: bool,
    pub(crate) filter: F,
}

impl<F> GatedFilter<F> {
    pub(crate) fn new(active: bool, filter: F) -> Self {
        Self { active, filter }
    }
}

impl<F: PreparedFilter> PreparedFilter for GatedFilter<F> {
    fn filter(&mut self, slots: Slice) -> Slice {
        if self.active {
            self.filter.filter(slots)
        } else {
            slots
        }
    }
}

impl<'w, F: Filter<'w>> Filter<'w> for GatedFilter<F> {
    type Prepared = GatedFilter<F::Prepared>;

    fn prepare(&'w self, archetype: &'w Archetype, change_tick: u32) -> Self::Prepared {
        GatedFilter {
            active: self.active,
            filter: self.filter.prepare(archetype, change_tick),
        }
    }

    fn matches(&self, arch: &Archetype) -> bool {
        !self.active || self.filter.matches(arch)
    }

    fn access(&self, id: ArchetypeId, arch: &Archetype) -> Vec<Access> {
        self.filter.access(id, arch)
    }
}

#[derive(Copy, Debug, Clone)]
/// Limit the batch size for a query
pub struct BatchSize(pub(crate) Slot);

impl PreparedFilter for BatchSize {
    fn filter(&mut self, slots: Slice) -> Slice {
        Slice::new(slots.start, slots.end.min(slots.start + self.0))
    }
}

impl<'w> Filter<'w> for BatchSize {
    type Prepared = BatchSize;

    fn prepare(&'w self, _: &'w Archetype, _: u32) -> Self::Prepared {
        if self.0 == 0 {
            panic!("Batch size of 0 will never yield");
        }
        *self
    }

    fn matches(&self, _: &Archetype) -> bool {
        true
    }

    fn access(&self, _: ArchetypeId, _: &Archetype) -> Vec<Access> {
        Default::default()
    }
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<$($ty, )*> StaticFilter for ($($ty,)*)
            where $($ty: StaticFilter,)*
        {
            fn static_matches(&self, arch: &Archetype) -> bool {
                $(self.$idx.static_matches(arch))||*
            }
        }

        impl<$($ty, )*> PreparedFilter for ($($ty,)*)
            where $($ty: PreparedFilter,)*
        {
            fn filter(&mut self, slots: Slice) -> Slice {
                let mut u = Slice::new(0, 0);

                $(
                    match u.union(&self.$idx.filter(slots)) {
                        Some(v) => { u = v }
                        None => { return u }
                    }
                )*

                u
            }
        }

        impl<'w, $($ty, )*> Filter<'w> for ($($ty,)*)
            where $($ty: Filter<'w>,)*
        {
            type Prepared       = ($($ty::Prepared,)*);

            fn prepare(&'w self, arch: &'w Archetype, change_tick: u32) -> Self::Prepared {
                ($(self.$idx.prepare(arch, change_tick),)*)
            }

            fn matches(&self, arch: &Archetype) -> bool {
                $(self.$idx.matches(arch))||*
            }

            fn access(&self, id: ArchetypeId, arch: &Archetype) -> Vec<Access> {
                [ $(self.$idx.access(id, arch),)* ].concat()
            }

        }
    };
}

tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }

#[cfg(test)]
mod tests {

    use alloc::string::String;
    use atomic_refcell::AtomicRefCell;
    use itertools::Itertools;

    use crate::{
        archetype::{Change, ChangeList},
        component, ChangeKind,
    };

    use super::*;

    #[test]
    fn filter() {
        let mut changes = ChangeList::default();

        changes.set(Change::new(Slice::new(40, 200), 1));
        changes.set(Change::new(Slice::new(70, 349), 2));
        changes.set(Change::new(Slice::new(560, 893), 5));
        changes.set(Change::new(Slice::new(39, 60), 6));
        changes.set(Change::new(Slice::new(784, 800), 7));
        changes.set(Change::new(Slice::new(945, 1139), 8));

        let changes = AtomicRefCell::new(changes);

        let filter = PreparedKindFilter::new(Some(changes.borrow()), 2);

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
        let mut changes_1 = ChangeList::default();
        let mut changes_2 = ChangeList::default();

        changes_1.set(Change::new(Slice::new(40, 65), 2));
        changes_1.set(Change::new(Slice::new(59, 80), 3));
        changes_1.set(Change::new(Slice::new(90, 234), 3));
        changes_2.set(Change::new(Slice::new(50, 70), 3));
        changes_2.set(Change::new(Slice::new(99, 210), 4));

        let a_map = changes_1.as_changed_set(1);
        let b_map = changes_2.as_changed_set(2);

        // eprintln!("ChangeList: \n  {changes_1:?}\n  {changes_2:?}");
        let changes_1 = AtomicRefCell::new(changes_1);
        let changes_2 = AtomicRefCell::new(changes_2);

        let slots = Slice::new(0, 1000);

        // Or
        let a = PreparedKindFilter::new(Some(changes_1.borrow()), 1);
        let b = PreparedKindFilter::new(Some(changes_2.borrow()), 2);

        let filter = PreparedOr { left: a, right: b };

        // Use a brute force BTreeSet for solving it
        let chunks_set = slots
            .iter()
            .filter(|v| a_map.contains(v) || b_map.contains(v))
            .collect_vec();

        let chunks = FilterIter::new(slots, filter).flatten().collect_vec();

        assert_eq!(chunks, chunks_set);

        // And

        let a = PreparedKindFilter::new(Some(changes_1.borrow()), 1);
        let b = PreparedKindFilter::new(Some(changes_2.borrow()), 2);

        let filter = PreparedAnd { left: a, right: b };

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

        let filter = ChangeFilter::new(a(), ChangeKind::Modified)
            & (ChangeFilter::new(b(), ChangeKind::Modified))
            | (ChangeFilter::new(c(), ChangeKind::Modified));

        // Mock changes
        let a_map = archetype
            .changes_mut(a().key())
            .unwrap()
            .set_modified(Change::new(Slice::new(9, 80), 2))
            .set_inserted(Change::new(Slice::new(65, 83), 4))
            .get(ChangeKind::Modified)
            .as_changed_set(1);

        let b_map = archetype
            .changes_mut(b().key())
            .unwrap()
            .set_modified(Change::new(Slice::new(16, 45), 2))
            .set_modified(Change::new(Slice::new(68, 85), 2))
            .get(ChangeKind::Modified)
            .as_changed_set(1);

        let c_map = archetype
            .changes_mut(c().key())
            .unwrap()
            .set_modified(Change::new(Slice::new(96, 123), 3))
            .get(ChangeKind::Modified)
            .as_changed_set(1);

        // Brute force
        let slots = Slice::new(0, 1000);
        let chunks_set = slots
            .iter()
            .filter(|v| (a_map.contains(v) && b_map.contains(v)) || (c_map.contains(v)))
            .collect_vec();

        let chunks = FilterIter::new(slots, filter.prepare(&archetype, 1))
            .flatten()
            .collect_vec();

        assert_eq!(chunks, chunks_set);
    }
}
