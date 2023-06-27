use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::{FetchAccessData, FetchPrepareData, FmtQuery, PreparedFetch},
    system::Access,
    Fetch, FetchItem,
};
use alloc::vec::Vec;
use core::{
    fmt::{self, Formatter},
    ops,
};

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

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.left.access(data, dst);
        self.right.access(data, dst);
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

        self.right.filter_slots(l)
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

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.0.access(data, dst)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "!{:?}", FmtQuery(&self.0))
    }
}

impl<'q, F> PreparedFetch<'q> for Not<Option<F>>
where
    F: PreparedFetch<'q>,
{
    type Item = ();

    #[inline]
    unsafe fn fetch(&mut self, _: usize) -> Self::Item {}

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.0 {
            let v = fetch.filter_slots(slots);

            slots.difference(v).unwrap()
        } else {
            slots
        }
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
