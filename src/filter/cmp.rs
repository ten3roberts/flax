//! Implements filters for component value comparisons.
//! The difference between these and a normal filter of if inside a for loop is
//! that entities **not** yielded will not be marked as modified.
//!
//! This is not possible using a normal if as the item is changed anyway.
//! An alternative may be a "modify guard", a Notify on Write, or NOW if you
//! want :P.

use core::{
    any::type_name,
    cmp::Ordering,
    fmt::{self, Debug},
};

use alloc::vec::Vec;

use crate::{
    archetype::{Slice, Slot},
    fetch::{FetchAccessData, FetchPrepareData, FmtQuery, PeekableFetch, PreparedFetch},
    Access, Fetch, FetchItem,
};

trait CmpMethod<L> {
    fn compare(&self, lhs: L) -> bool;
}

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct Less<R>(pub(crate) R);
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct Greater<R>(pub(crate) R);
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct Equal<R>(pub(crate) R);
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct LessEq<R>(pub(crate) R);
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct GreaterEq<R>(pub(crate) R);

impl<L, R> CmpMethod<L> for Less<R>
where
    L: for<'x> PartialOrd<&'x R>,
{
    fn compare(&self, lhs: L) -> bool {
        matches!(lhs.partial_cmp(&&self.0), Some(Ordering::Less))
    }
}

impl<L, R> CmpMethod<L> for Greater<R>
where
    L: for<'x> PartialOrd<&'x R>,
{
    fn compare(&self, lhs: L) -> bool {
        matches!(lhs.partial_cmp(&&self.0), Some(Ordering::Greater))
    }
}

impl<L, R> CmpMethod<L> for GreaterEq<R>
where
    L: for<'x> PartialOrd<&'x R>,
{
    fn compare(&self, lhs: L) -> bool {
        matches!(
            lhs.partial_cmp(&&self.0),
            Some(Ordering::Greater | Ordering::Equal)
        )
    }
}

impl<L, R> CmpMethod<L> for LessEq<R>
where
    L: for<'x> PartialOrd<&'x R>,
{
    fn compare(&self, lhs: L) -> bool {
        matches!(
            lhs.partial_cmp(&&self.0),
            Some(Ordering::Less | Ordering::Equal)
        )
    }
}

impl<L, R> CmpMethod<L> for Equal<R>
where
    L: for<'x> PartialEq<&'x R>,
{
    fn compare(&self, lhs: L) -> bool {
        lhs.eq(&&self.0)
    }
}

impl<T, F> CmpMethod<T> for F
where
    F: Fn(T) -> bool,
{
    fn compare(&self, lhs: T) -> bool {
        (self)(lhs)
    }
}

#[derive(Debug, Clone)]
/// Filter which allows comparison to peeked items
pub struct Cmp<F, C> {
    fetch: F,
    method: C,
}

impl<F, C> Cmp<F, C> {
    /// Creates a new comparison filter
    pub fn new(fetch: F, method: C) -> Self {
        Self { fetch, method }
    }
}

impl<'q, F: FetchItem<'q>, M> FetchItem<'q> for Cmp<F, M> {
    type Item = F::Item;
}

impl<'w, F, M> Fetch<'w> for Cmp<F, M>
where
    F: Fetch<'w>,
    F::Prepared: for<'x> PeekableFetch<'x>,
    M: for<'x> CmpMethod<<F::Prepared as PeekableFetch<'x>>::Peek> + 'w,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = PreparedCmp<'w, F::Prepared, M>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedCmp {
            fetch: self.fetch.prepare(data)?,
            method: &self.method,
        })
    }

    fn filter_arch(&self, arch: &crate::Archetype) -> bool {
        self.fetch.filter_arch(arch)
    }

    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        self.fetch.access(data)
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} <=> {}", FmtQuery(&self.fetch), &type_name::<M>())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        self.fetch.searcher(searcher)
    }
}

pub struct PreparedCmp<'w, F, M> {
    fetch: F,
    method: &'w M,
}

impl<'q, 'w, F, M> PreparedFetch<'q> for PreparedCmp<'w, F, M>
where
    F: PreparedFetch<'q> + for<'x> PeekableFetch<'x>,
    M: for<'x> CmpMethod<<F as PeekableFetch<'x>>::Peek> + 'w,
{
    type Item = <F as PreparedFetch<'q>>::Item;

    #[inline]
    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        self.fetch.fetch(slot)
    }

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let mut cmp = |slot: Slot| {
            let lhs = unsafe { self.fetch.peek(slot) };
            self.method.compare(lhs)
        };

        // Find the first slot which yield true
        let first = match slots.iter().position(&mut cmp) {
            Some(v) => v,
            None => return Slice::empty(),
        };

        let count = slots
            .iter()
            .skip(first)
            .take_while(|&slot| cmp(slot))
            .count();

        Slice {
            start: slots.start + first,
            end: slots.start + first + count,
        }
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice) {
        self.fetch.set_visited(slots)
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{component, entity_ids, name, BatchSpawn, FetchExt, Query, World};

    #[test]
    fn cmp_mut() {
        let mut batch = BatchSpawn::new(128);

        component! {
            a: i32,
        }

        batch.set(a(), (0..10).cycle()).unwrap();
        batch.set(name(), (0i32..).map(|v| v.to_string())).unwrap();

        let mut world = World::new();
        let ids = batch.spawn(&mut world);

        let mut changed = Query::new(entity_ids()).filter(a().modified());

        assert_eq!(changed.collect_vec(&world), ids);

        let mut query = Query::new(a().as_mut().opt().cmp(|v: Option<&i32>| v > Some(&3)));
        for item in query.borrow(&world).iter().flatten() {
            *item *= -1;
        }

        let changed_ids = ids
            .iter()
            .enumerate()
            .filter(|&v| v.0 % 10 > 3)
            .map(|v| *v.1)
            .collect_vec();

        assert_eq!(changed.collect_vec(&world), changed_ids);
    }
}
