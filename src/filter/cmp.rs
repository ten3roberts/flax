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
    fetch::{
        FetchAccessData, FetchPrepareData, FmtQuery, PreparedFetch, ReadOnlyFetch, TransformFetch,
    },
    system::Access,
    Fetch, FetchItem,
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
    F::Prepared: for<'x> ReadOnlyFetch<'x>,
    M: for<'x> CmpMethod<<F::Prepared as PreparedFetch<'x>>::Item> + 'w,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = PreparedCmp<'w, F::Prepared, M>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedCmp {
            fetch: self.fetch.prepare(data)?,
            method: &self.method,
        })
    }

    fn filter_arch(&self, arch: &crate::archetype::Archetype) -> bool {
        self.fetch.filter_arch(arch)
    }

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.fetch.access(data, dst)
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

impl<'w, 'q, F, M> ReadOnlyFetch<'q> for PreparedCmp<'w, F, M>
where
    F: for<'x> ReadOnlyFetch<'x>,
    M: for<'x> CmpMethod<<F as PreparedFetch<'x>>::Item> + 'w,
{
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
        self.fetch.fetch_shared(slot)
    }

    unsafe fn fetch_shared_chunk(batch: &Self::Chunk, slot: Slot) -> Self::Item {
        F::fetch_shared_chunk(batch, slot)
    }
}

impl<'w, 'q, Q, M> PreparedFetch<'q> for PreparedCmp<'w, Q, M>
where
    Q: for<'x> ReadOnlyFetch<'x>,
    M: for<'x> CmpMethod<<Q as PreparedFetch<'x>>::Item> + 'w,
{
    type Item = <Q as PreparedFetch<'q>>::Item;

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let slots = self.fetch.filter_slots(slots);

        let mut cmp = |slot: Slot| {
            let lhs = unsafe { self.fetch.fetch_shared(slot) };
            self.method.compare(lhs)
        };

        // Find the first slot which yield true
        let first = slots.iter().position(&mut cmp).unwrap_or(slots.len());

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

    type Chunk = <Q as PreparedFetch<'q>>::Chunk;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.fetch.create_chunk(slots)
    }

    #[inline]
    unsafe fn fetch_next(batch: &mut Self::Chunk) -> Self::Item {
        Q::fetch_next(batch)
    }
}

impl<K, F, C> TransformFetch<K> for Cmp<F, C>
where
    F: TransformFetch<K>,
{
    type Output = Cmp<F::Output, C>;

    fn transform_fetch(self, method: K) -> Self::Output {
        Cmp {
            fetch: self.fetch.transform_fetch(method),
            method: self.method,
        }
    }
}

#[cfg(test)]
mod test {
    use alloc::string::ToString;
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{component, entity_ids, name, BatchSpawn, CommandBuffer, FetchExt, Query, World};

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

        let mut cmd = CommandBuffer::new();
        let mut query = Query::new((entity_ids(), a().opt().cmp(|v: Option<&i32>| v > Some(&3))));
        for (id, item) in query.borrow(&world).iter() {
            if let Some(item) = item {
                cmd.set(id, a(), item * -1);
            }
        }

        cmd.apply(&mut world).unwrap();

        let changed_ids = ids
            .iter()
            .enumerate()
            .filter(|&v| v.0 % 10 > 3)
            .map(|v| *v.1)
            .collect_vec();

        assert_eq!(changed.collect_vec(&world), changed_ids);
    }

    #[test]
    fn cmp_nested() {
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

        let mut cmd = CommandBuffer::new();
        let mut query = Query::new((entity_ids(), a().gt(3).lt(7)));
        for (id, item) in query.borrow(&world).iter() {
            cmd.set(id, a(), item * -1);
        }

        cmd.apply(&mut world).unwrap();

        let changed_ids = ids
            .iter()
            .enumerate()
            .filter(|&v| v.0 % 10 > 3 && v.0 % 10 < 7)
            .map(|v| *v.1)
            .collect_vec();

        assert_eq!(changed.collect_vec(&world), changed_ids);
    }
}
