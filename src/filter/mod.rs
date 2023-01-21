mod change;
mod cmp;
use alloc::vec::Vec;

use core::{
    any::type_name,
    fmt::{self, Formatter},
    iter::FusedIterator,
    ops,
};

use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::{FetchAccessData, FetchPrepareData, FmtQuery, PreparedFetch},
    Access, ArchetypeSearcher, ComponentKey, Entity, Fetch, FetchItem,
};

pub use change::*;
pub use cmp::{Cmp, Equal, Greater, GreaterEq, Less, LessEq};

macro_rules! gen_bitops {
    ($ty:ident[$($p: tt),*]) => {
        impl<R, $($p),*> ops::BitOr<R> for $ty<$($p),*>
        {
            type Output = Or<(Self, R)>;

            fn bitor(self, rhs: R) -> Self::Output {
                Or((self, rhs))
            }
        }

        impl<R, $($p),*> ops::BitAnd<R> for $ty<$($p),*>
        {
            type Output = And<Self, R>;

            fn bitand(self, rhs: R) -> Self::Output {
                And::new(self, rhs)
            }
        }

        impl<$($p),*> ops::Not for $ty<$($p),*>
        {
            type Output = Not<Self>;

            fn not(self) -> Self::Output {
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

#[derive(Debug, Clone)]
/// Wraps a query by a filtering query
pub struct Filtered<Q, F> {
    pub(crate) fetch: Q,
    pub(crate) filter: F,
}

impl<Q, F> Filtered<Q, F> {
    pub(crate) fn new(fetch: Q, filter: F) -> Self {
        Self { fetch, filter }
    }
}

impl<'w, Q, F> FetchItem<'w> for Filtered<Q, F>
where
    Q: FetchItem<'w>,
{
    type Item = Q::Item;
}

impl<'w, Q, F> Fetch<'w> for Filtered<Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Only F is fetched
    const MUTABLE: bool = Q::MUTABLE;

    type Prepared = Filtered<Q::Prepared, F::Prepared>;

    #[inline]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(Filtered {
            fetch: self.fetch.prepare(data)?,
            filter: self.filter.prepare(data)?,
        })
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        self.fetch.filter_arch(arch) && self.filter.filter_arch(arch)
    }

    #[inline]
    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        let mut v = self.fetch.access(data);
        v.append(&mut self.filter.access(data));
        v
    }

    #[inline]
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.fetch.describe(f)?;
        write!(f, " & ")?;
        self.filter.describe(f)
    }

    #[inline]
    fn searcher(&self, searcher: &mut ArchetypeSearcher) {
        self.fetch.searcher(searcher);
        self.filter.searcher(searcher);
    }
}

impl<'q, Q, F> PreparedFetch<'q> for Filtered<Q, F>
where
    Q: PreparedFetch<'q>,
    F: PreparedFetch<'q>,
{
    type Item = Q::Item;

    #[inline]
    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        self.fetch.fetch(slot)
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let l = self.fetch.filter_slots(slots);
        let r = self.filter.filter_slots(slots);
        dbg!(l, r);

        let i = l.intersect(&r);
        if i.is_empty() {
            // Go again but start with the highest bound
            // This is caused by one of the sides being past the end of the
            // other slice. As such, force the slice lagging behind to catch up
            // to the upper floor
            let common_start = l.start.max(r.start).clamp(slots.start, slots.end);

            let slots = Slice::new(common_start, slots.end);
            let l = self.fetch.filter_slots(slots);
            let r = self.filter.filter_slots(slots);
            l.intersect(&r)
        } else {
            i
        }
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice) {
        self.fetch.set_visited(slots)
    }
}

gen_bitops! {
    All[];
    And[A,B];
    BatchSize[];
    ChangeFilter[T];
    Nothing[];
    Or[T];
    RemovedFilter[T];
    WithObject[];
    WithRelation[];
    With[];
    WithoutRelation[];
    Without[];
    Cmp[A,B];
    Slice[];
}

#[derive(Debug, Clone)]
/// And combinator
pub struct And<L, R> {
    pub(crate) left: L,
    pub(crate) right: R,
}

impl<L, R> And<L, R> {
    /// Creates a new and filter
    pub fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<'q, L, R> FetchItem<'q> for And<L, R>
where
    L: FetchItem<'q>,
    R: FetchItem<'q>,
{
    type Item = (L::Item, R::Item);
}

impl<'w, L, R> Fetch<'w> for And<L, R>
where
    L: Fetch<'w>,
    R: Fetch<'w>,
{
    const MUTABLE: bool = false;

    type Prepared = And<L::Prepared, R::Prepared>;

    #[inline]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(And {
            left: self.left.prepare(data)?,
            right: self.right.prepare(data)?,
        })
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        self.left.filter_arch(arch) && self.right.filter_arch(arch)
    }

    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        let mut res = self.left.access(data);
        res.append(&mut self.right.access(data));
        res
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.left.describe(f)?;
        f.write_str(" & ")?;
        self.right.describe(f)?;

        Ok(())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        self.left.searcher(searcher);
        self.right.searcher(searcher);
    }
}

impl<'q, L, R> PreparedFetch<'q> for And<L, R>
where
    L: PreparedFetch<'q>,
    R: PreparedFetch<'q>,
{
    type Item = (L::Item, R::Item);

    #[inline]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        (self.left.fetch(slot), self.right.fetch(slot))
    }

    fn set_visited(&mut self, slots: Slice) {
        self.left.set_visited(slots);
        self.right.set_visited(slots);
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let l = self.left.filter_slots(slots);
        let r = self.right.filter_slots(slots);

        let i = l.intersect(&r);
        if i.is_empty() {
            // Go again but start with the highest bound
            // This is caused by one of the sides being past the end of the
            // other slice. As such, force the slice lagging behind to catch up
            // to the upper floor
            let common_start = l.start.max(r.start).clamp(slots.start, slots.end);

            let slots = Slice::new(common_start, slots.end);
            let l = self.left.filter_slots(slots);
            let r = self.right.filter_slots(slots);
            l.intersect(&r)
        } else {
            i
        }
    }
}

#[derive(Debug, Clone)]
/// Or filter combinator
pub struct Or<T>(pub T);

#[derive(Debug, Clone)]
/// Negate a filter
pub struct Not<T>(pub T);

impl<'q, T> FetchItem<'q> for Not<T> {
    type Item = ();
}

impl<'w, T> Fetch<'w> for Not<T>
where
    T: Fetch<'w>,
{
    const MUTABLE: bool = true;

    type Prepared = Not<Option<T::Prepared>>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(Not(self.0.prepare(data)))
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        !self.0.filter_arch(arch)
    }

    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        self.0.access(data)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "!{:?}", FmtQuery(&self.0))
    }

    fn searcher(&self, _: &mut ArchetypeSearcher) {}
}

impl<'q, F> PreparedFetch<'q> for Not<Option<F>>
where
    F: PreparedFetch<'q>,
{
    type Item = ();

    #[inline]
    unsafe fn fetch(&mut self, _: usize) -> Self::Item {}

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let v = self.0.filter_slots(slots);

        slots.difference(v).unwrap()
    }
}

impl<R, T> ops::BitOr<R> for Not<T> {
    type Output = Or<(Self, R)>;

    fn bitor(self, rhs: R) -> Self::Output {
        Or((self, rhs))
    }
}

impl<R, T> ops::BitAnd<R> for Not<T> {
    type Output = (Self, R);

    fn bitand(self, rhs: R) -> Self::Output {
        (self, rhs)
    }
}

impl<T> ops::Not for Not<T> {
    type Output = T;

    fn not(self) -> Self::Output {
        self.0
    }
}

#[derive(Debug, Clone)]
/// A filter that yields, well, nothing
pub struct Nothing;

impl<'q> FetchItem<'q> for Nothing {
    type Item = ();
}

impl<'q> FetchItem<'q> for All {
    type Item = ();
}

impl<'a> Fetch<'a> for Nothing {
    const MUTABLE: bool = false;

    type Prepared = bool;

    #[inline(always)]
    fn prepare(&self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(false)
    }

    #[inline(always)]
    fn filter_arch(&self, _: &Archetype) -> bool {
        false
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "false")
    }
}

/// Yields all entities
#[derive(Debug, Clone)]
pub struct All;

impl<'w> Fetch<'w> for All {
    const MUTABLE: bool = false;

    type Prepared = bool;

    fn prepare(&'w self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(true)
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "true")
    }
}

impl<'q> FetchItem<'q> for Slice {
    type Item = ();
}

impl<'w> Fetch<'w> for Slice {
    const MUTABLE: bool = false;

    type Prepared = Self;

    fn prepare(&'w self, _: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(*self)
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "slice {:?}", self)
    }
}

impl<'q> PreparedFetch<'q> for Slice {
    type Item = ();

    #[inline]
    unsafe fn fetch(&mut self, _: usize) -> Self::Item {}

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        self.intersect(&slots)
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

impl<'q, F> Iterator for FilterIter<F>
where
    F: PreparedFetch<'q>,
{
    type Item = Slice;

    fn next(&mut self) -> Option<Self::Item> {
        if self.slots.is_empty() {
            return None;
        }

        // Safety
        // The yielded slots are split off of `self.slots`
        let cur = unsafe { self.filter.filter_slots(self.slots) };

        if cur.is_empty() {
            None
        } else {
            let (_l, m, r) = {
                match self.slots.split_with(&cur) {
                    Some(val) => val,
                    None => panic!("Return value of filter must be a subset of slots. Got: slots: {:?} cur: {cur:?}" ,self.slots),
                }
            };

            self.slots = r;
            Some(m)
        }
    }
}

impl<'q, F: PreparedFetch<'q>> FusedIterator for FilterIter<F> {}

#[derive(Debug, Clone)]
/// Fetch which only yields if the entity has the specified component
pub struct With {
    pub(crate) component: ComponentKey,
    pub(crate) name: &'static str,
}

impl<'q> FetchItem<'q> for With {
    type Item = ();
}

impl<'a> Fetch<'a> for With {
    const MUTABLE: bool = false;

    type Prepared = bool;

    fn prepare(&self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if self.filter_arch(data.arch) {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.component)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "with {}", self.name)
    }
}

#[derive(Debug, Clone)]
/// Opposite of [crate::Without]
pub struct Without {
    pub(crate) component: ComponentKey,
    pub(crate) name: &'static str,
}

impl<'q> FetchItem<'q> for Without {
    type Item = ();
}

impl<'w> Fetch<'w> for Without {
    const MUTABLE: bool = false;

    type Prepared = bool;

    fn prepare(&self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if self.filter_arch(data.arch) {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        !arch.has(self.component)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "without {}", self.name)
    }
}

#[derive(Debug, Clone)]
/// Yields all entities with the relation of the specified kind
pub(crate) struct WithObject {
    pub(crate) object: Entity,
}

impl<'q> FetchItem<'q> for WithObject {
    type Item = ();
}

impl<'w> Fetch<'w> for WithObject {
    const MUTABLE: bool = false;

    type Prepared = bool;

    fn prepare(&self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if self.filter_arch(data.arch) {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.components().any(|v| {
            if let Some(v) = v.key().object {
                if v == self.object {
                    return true;
                }
            }

            false
        })
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "with (*)({})", self.object)
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

impl<'q, F> FetchItem<'q> for ArchetypeFilter<F> {
    type Item = ();
}

impl<'w, F: Fn(&Archetype) -> bool> Fetch<'w> for ArchetypeFilter<F> {
    const MUTABLE: bool = false;
    type Prepared = bool;

    fn prepare(&'w self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if self.filter_arch(data.arch) {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        (self.0)(arch)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "archetype_filter {}", &type_name::<F>())
    }
}

#[derive(Debug, Clone)]
/// Yields all entities with the relation of the specified kind
pub struct WithRelation {
    pub(crate) relation: Entity,
    pub(crate) name: &'static str,
}

impl<'q> FetchItem<'q> for WithRelation {
    type Item = ();
}

impl<'w> Fetch<'w> for WithRelation {
    const MUTABLE: bool = false;
    type Prepared = bool;

    fn prepare(&self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if self.filter_arch(data.arch) {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.relations_like(self.relation).next().is_some()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "with {}(*)", self.name)
    }
}

#[derive(Debug, Clone)]
/// Opposite of [crate::Without]
pub struct WithoutRelation {
    pub(crate) relation: Entity,
    pub(crate) name: &'static str,
}

impl<'q> FetchItem<'q> for WithoutRelation {
    type Item = ();
}

impl<'a> Fetch<'a> for WithoutRelation {
    const MUTABLE: bool = false;

    type Prepared = bool;

    fn prepare(&self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if self.filter_arch(data.arch) {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.relations_like(self.relation).next().is_none()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "without {}(*)", self.name)
    }
}

impl<'q> FetchItem<'q> for bool {
    type Item = ();
}

impl<'w> Fetch<'w> for bool {
    const MUTABLE: bool = false;

    type Prepared = Self;

    #[inline(always)]
    fn prepare(&'w self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(*self)
    }

    #[inline(always)]
    fn filter_arch(&self, _: &Archetype) -> bool {
        *self
    }

    #[inline(always)]
    fn access(&self, _: FetchAccessData) -> Vec<Access> {
        Default::default()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<'q> PreparedFetch<'q> for bool {
    type Item = ();

    #[inline]
    unsafe fn fetch(&mut self, _: usize) -> Self::Item {}

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if *self {
            slots
        } else {
            Slice::empty()
        }
    }
}

/// Allows a fetch to be used by reference.
pub struct RefFetch<'a, F>(pub(crate) &'a F);

impl<'a, F> Copy for RefFetch<'a, F> {}
impl<'a, F> Clone for RefFetch<'a, F> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl<'a, 'q, F> FetchItem<'q> for RefFetch<'a, F>
where
    F: FetchItem<'q>,
{
    type Item = F::Item;
}

impl<'a, 'w, F> Fetch<'w> for RefFetch<'a, F>
where
    F: Fetch<'w>,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = F::Prepared;

    #[inline]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        (*self.0).prepare(data)
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        (*self.0).filter_arch(arch)
    }

    #[inline]
    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        (*self.0).access(data)
    }

    #[inline]
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        (*self.0).describe(f)
    }

    #[inline]
    fn searcher(&self, searcher: &mut ArchetypeSearcher) {
        (*self.0).searcher(searcher)
    }
}

impl<'a, 'q, F> FetchItem<'q> for &'a F
where
    F: FetchItem<'q>,
{
    type Item = F::Item;
}

impl<'a, 'w, F> Fetch<'w> for &'a F
where
    'a: 'w,
    F: Fetch<'w>,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = F::Prepared;

    #[inline]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        (*self).prepare(data)
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        (*self).filter_arch(arch)
    }

    #[inline]
    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        (*self).access(data)
    }

    #[inline]
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        (*self).describe(f)
    }

    #[inline]
    fn searcher(&self, searcher: &mut ArchetypeSearcher) {
        (*self).searcher(searcher)
    }
}

/// Limit the batch size for a query
#[derive(Copy, Debug, Clone)]
pub struct BatchSize(pub(crate) Slot);

impl<'q> PreparedFetch<'q> for BatchSize {
    type Item = ();

    #[inline]
    unsafe fn fetch(&mut self, _: usize) -> Self::Item {}

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        Slice::new(slots.start, slots.end.min(slots.start + self.0))
    }
}

impl<'q> FetchItem<'q> for BatchSize {
    type Item = ();
}

impl<'w> Fetch<'w> for BatchSize {
    const MUTABLE: bool = false;

    type Prepared = Self;

    fn prepare(&'w self, _: FetchPrepareData) -> Option<Self::Prepared> {
        if self.0 == 0 {
            panic!("Batch size of 0 will never yield");
        }
        Some(*self)
    }

    #[inline]
    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "batch {}", self.0)
    }
}

#[doc(hidden)]
pub trait StaticFilter {
    fn filter_static(&self, arch: &Archetype) -> bool;
}

impl<F> StaticFilter for F
where
    for<'x> F: Fetch<'x>,
{
    fn filter_static(&self, arch: &Archetype) -> bool {
        <F as Fetch>::filter_arch(self, arch)
    }
}

#[cfg(test)]
mod tests {

    use alloc::string::String;
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{
        archetype::{Change, ChangeList},
        component, ArchetypeId, ChangeKind, World,
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

        let filter = PreparedKindFilter::new((), changes.as_slice(), 2);

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
        let slots = Slice::new(0, 1000);

        // Or
        let a = PreparedKindFilter::new((), changes_1.as_slice(), 1);
        let b = PreparedKindFilter::new((), changes_2.as_slice(), 2);

        let filter = Or((Some(a), Some(b)));

        // Use a brute force BTreeSet for solving it
        let chunks_set = slots
            .iter()
            .filter(|v| a_map.contains(v) || b_map.contains(v))
            .collect_vec();

        let chunks = FilterIter::new(slots, filter).flatten().collect_vec();

        assert_eq!(chunks, chunks_set);

        // And

        let a = PreparedKindFilter::new((), changes_1.as_slice(), 1);
        let b = PreparedKindFilter::new((), changes_2.as_slice(), 2);

        let filter = And { left: a, right: b };

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

        let world = World::new();
        let chunks = FilterIter::new(
            slots,
            filter.prepare(FetchPrepareData {
                world: &world,
                arch: &archetype,
                arch_id: ArchetypeId::MAX,
                old_tick: 0,
                new_tick: 1,
            }),
        )
        .flatten()
        .collect_vec();

        assert_eq!(chunks, chunks_set);
    }
}
