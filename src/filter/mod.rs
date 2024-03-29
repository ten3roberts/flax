mod change;
mod cmp;
mod constant;
mod set;

use alloc::vec::Vec;
use core::{
    any::type_name,
    fmt::{self, Formatter},
    iter::FusedIterator,
    ops,
};

use crate::{
    archetype::{Archetype, Slice, Slot},
    component::ComponentKey,
    components::component_info,
    fetch::{FetchAccessData, FetchPrepareData, PreparedFetch},
    system::Access,
    ArchetypeSearcher, Entity, Fetch, FetchItem,
};

pub use change::ChangeFilter;
pub use cmp::{Cmp, Equal, Greater, GreaterEq, Less, LessEq};
pub(crate) use constant::NoEntities;
pub use constant::{All, Nothing};
pub use set::{And, Not, Or, Union};

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
                And(self, rhs)
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
    pub(crate) include_components: bool,
}

impl<Q, F> Filtered<Q, F> {
    pub(crate) fn new(fetch: Q, filter: F, include_components: bool) -> Self {
        Self {
            fetch,
            filter,
            include_components,
        }
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
            include_components: self.include_components,
        })
    }

    #[inline]
    fn filter_arch(&self, data: FetchAccessData<'_>) -> bool {
        self.fetch.filter_arch(data)
            && self.filter.filter_arch(data)
            && (!data.arch.has(component_info().key()) || self.include_components)
    }

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.fetch.access(data, dst);
        self.filter.access(data, dst);
    }

    #[inline]
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.fetch.describe(f)?;
        write!(f, ", ")?;
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
    const HAS_FILTER: bool = Q::HAS_FILTER || F::HAS_FILTER;

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let l = self.fetch.filter_slots(slots);
        self.filter.filter_slots(l)
    }

    type Chunk = Q::Chunk;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.fetch.create_chunk(slots)
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        Q::fetch_next(chunk)
    }
}

gen_bitops! {
    All[];
    And[A,B];
    BatchSize[];
    ChangeFilter[T];
    Nothing[];
    Or[T];
    WithTarget[];
    WithRelation[];
    With[];
    WithoutRelation[];
    Without[];
    Cmp[A,B];
}

#[derive(Debug, Clone)]
/// Iterator which yields slices which match the underlying filter
pub struct FilterIter<Q> {
    pub(crate) fetch: Q,
    // Remaining slots
    slots: Slice,
}

impl<Q> FilterIter<Q> {
    /// Creates a new filter iterator visiting the `slot` of the same archetype
    /// as `F`
    #[inline(always)]
    pub fn new(slots: Slice, fetch: Q) -> Self {
        Self { slots, fetch }
    }
}

impl<'q, Q> Iterator for FilterIter<Q>
where
    Q: PreparedFetch<'q>,
{
    type Item = Slice;

    fn next(&mut self) -> Option<Self::Item> {
        next_slice(&mut self.slots, &mut self.fetch)
    }
}

pub(crate) fn next_slice<'a, Q: PreparedFetch<'a>>(
    slots: &mut Slice,
    fetch: &mut Q,
) -> Option<Slice> {
    if slots.is_empty() {
        return None;
    }

    while !slots.is_empty() {
        // Safety
        // The yielded slots are split off of `self.slots`
        let cur = unsafe { fetch.filter_slots(*slots) };

        let (_l, m, r) = slots
            .split_with(&cur)
            .expect("Return value of filter must be a subset of `slots");

        assert_eq!(cur, m);

        *slots = r;

        if !m.is_empty() {
            return Some(m);
        }
    }

    None
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

    type Prepared = All;

    fn prepare(&self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if data.arch.has(self.component) {
            Some(All)
        } else {
            None
        }
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        data.arch.has(self.component)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "with {}", self.name)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl StaticFilter for With {
    fn filter_static(&self, arch: &Archetype) -> bool {
        arch.has(self.component)
    }
}

#[derive(Debug, Clone)]
/// Opposite of [crate::filter::Without]
pub struct Without {
    pub(crate) component: ComponentKey,
    pub(crate) name: &'static str,
}

impl<'q> FetchItem<'q> for Without {
    type Item = ();
}

impl<'w> Fetch<'w> for Without {
    const MUTABLE: bool = false;

    type Prepared = All;

    fn prepare(&self, data: FetchPrepareData) -> Option<Self::Prepared> {
        if !data.arch.has(self.component) {
            Some(All)
        } else {
            None
        }
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        !data.arch.has(self.component)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "without {}", self.name)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl StaticFilter for Without {
    fn filter_static(&self, arch: &Archetype) -> bool {
        !arch.has(self.component)
    }
}

#[derive(Debug, Clone)]
/// Yields all entities with the relation of the specified kind
pub(crate) struct WithTarget {
    pub(crate) target: Entity,
}

impl<'q> FetchItem<'q> for WithTarget {
    type Item = ();
}

impl<'w> Fetch<'w> for WithTarget {
    const MUTABLE: bool = false;

    type Prepared = All;

    fn prepare(&self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(All)
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.filter_static(data.arch)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "with (*)({})", self.target)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl StaticFilter for WithTarget {
    fn filter_static(&self, arch: &Archetype) -> bool {
        arch.components().keys().any(|v| {
            if let Some(v) = v.target {
                if v == self.target {
                    return true;
                }
            }

            false
        })
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
    type Prepared = All;

    fn prepare(&'w self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(All)
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        (self.0)(data.arch)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "archetype_filter {}", &type_name::<F>())
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
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
    type Prepared = All;

    fn prepare(&self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(All)
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.filter_static(data.arch)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "with {}(*)", self.name)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl StaticFilter for WithRelation {
    fn filter_static(&self, arch: &Archetype) -> bool {
        arch.relations_like(self.relation).next().is_some()
    }
}

#[derive(Debug, Clone)]
/// Opposite of [crate::filter::Without]
pub struct WithoutRelation {
    pub(crate) relation: Entity,
    pub(crate) name: &'static str,
}

impl<'q> FetchItem<'q> for WithoutRelation {
    type Item = ();
}

impl<'a> Fetch<'a> for WithoutRelation {
    const MUTABLE: bool = false;

    type Prepared = All;

    fn prepare(&self, _: FetchPrepareData) -> Option<Self::Prepared> {
        Some(All)
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.filter_static(data.arch)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "without {}(*)", self.name)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

impl StaticFilter for WithoutRelation {
    fn filter_static(&self, arch: &Archetype) -> bool {
        arch.relations_like(self.relation).next().is_none()
    }
}

/// Allows a fetch to be used by reference.
pub struct RefFetch<'a, F>(pub(crate) &'a F);

impl<'a, F> Copy for RefFetch<'a, F> {}

impl<'a, F> Clone for RefFetch<'a, F> {
    fn clone(&self) -> Self {
        *self
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
    fn filter_arch(&self, data: FetchAccessData) -> bool {
        (*self.0).filter_arch(data)
    }

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        (*self.0).access(data, dst)
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
    fn filter_arch(&self, data: FetchAccessData) -> bool {
        (*self).filter_arch(data)
    }

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        (*self).access(data, dst)
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
    type Chunk = ();
    const HAS_FILTER: bool = false;

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        Slice::new(slots.start, slots.end.min(slots.start + self.0))
    }

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {}

    #[inline]
    unsafe fn fetch_next(_: &mut Self::Chunk) -> Self::Item {}
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
    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "batch {}", self.0)
    }

    #[inline]
    fn access(&self, _: FetchAccessData, _: &mut Vec<Access>) {}
}

#[doc(hidden)]
pub trait StaticFilter {
    fn filter_static(&self, arch: &Archetype) -> bool;
}

// impl<F> StaticFilter for F
// where
//     for<'x> F: Fetch<'x>,
// {
//     fn filter_static(&self, data: FetchAccessData) -> bool {
//         <F as Fetch>::filter_arch(self, data)
//     }
// }

#[cfg(test)]
mod tests {

    use alloc::string::String;
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{
        archetype::{ArchetypeId, Change, ChangeKind, ChangeList},
        component,
        filter::change::ChangeFetch,
        World,
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

        let filter = ChangeFetch::new(changes.as_slice(), 2);

        // The whole "archetype"
        let slots = Slice::new(0, 1238);

        let chunks = FilterIter::new(slots, filter).collect_vec();

        assert_eq!(
            chunks,
            [
                Slice::new(39, 60),
                Slice::new(560, 784),
                Slice::new(784, 800),
                Slice::new(800, 893),
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

        let slots = Slice::new(0, 1000);

        // Or
        let a = ChangeFetch::new(changes_1.as_slice(), 1);
        let b = ChangeFetch::new(changes_2.as_slice(), 2);

        let filter = Or((Some(a), Some(b)));

        // Use a brute force BTreeSet for solving it
        let chunks_set = slots
            .iter()
            .filter(|v| a_map.contains(v) || b_map.contains(v))
            .collect_vec();

        let chunks = FilterIter::new(slots, filter).flatten().collect_vec();

        assert_eq!(chunks, chunks_set);

        // And

        let a = ChangeFetch::new(changes_1.as_slice(), 1);
        let b = ChangeFetch::new(changes_2.as_slice(), 2);

        let filter = And(a, b);

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

        let mut archetype = Archetype::new([a().desc(), b().desc(), c().desc()]);

        let filter = (ChangeFilter::new(a(), ChangeKind::Modified)
            & ChangeFilter::new(b(), ChangeKind::Modified))
            | (ChangeFilter::new(c(), ChangeKind::Modified));

        // Mock changes
        let a_map = archetype
            .changes_mut(a().key())
            .unwrap()
            .set_modified(Change::new(Slice::new(9, 80), 2))
            .set_added(Change::new(Slice::new(65, 83), 4))
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
            filter
                .prepare(FetchPrepareData {
                    world: &world,
                    arch: &archetype,
                    arch_id: ArchetypeId::MAX,
                    old_tick: 0,
                    new_tick: 1,
                })
                .unwrap(),
        )
        .flatten()
        .collect_vec();

        assert_eq!(chunks, chunks_set);
    }
}
