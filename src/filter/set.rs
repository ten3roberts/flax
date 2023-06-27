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

/// Unionized the slot-level filter of two fetches, but requires the individual fetches to still
/// match.
///
/// This allows the filters to return fetch items side by side like the wrapped
/// fetch would, since all constituent fetches are satisfied, but not necessarily all their entities.
///
/// This is most useful for change queries, where you care about about *any* change, but still
/// require the entity to have all the components, and have them returned despite not all having
/// changed.
pub struct Union<T>(pub T);

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        // Union
        impl<'w, $($ty: Fetch<'w>,)*> Fetch<'w> for Union<($($ty,)*)> {
            const MUTABLE: bool = $($ty::MUTABLE )||*;

            type Prepared = Union<($($ty::Prepared,)*)>;

            fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
                let inner = &self.0;
                Some(Union(($(inner.$idx.prepare(data)?,)*)))
            }

            fn filter_arch(&self, arch: &crate::archetype::Archetype) -> bool {
                let inner = &self.0;
                $(inner.$idx.filter_arch(arch))&&*
            }

            fn access(&self, data: super::FetchAccessData, dst: &mut alloc::vec::Vec<crate::system::Access>) {
                let inner = &self.0;
                $(inner.$idx.access(data, dst);)*
            }

            fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let inner = &self.0;
                let mut s = f.debug_tuple("Union");
                $(s.field(&FmtQuery(&inner.$idx));)*

                s.finish()
            }
        }


        impl<'w, $($ty: PreparedFetch<'w>,)* > PreparedFetch<'w> for Union<($($ty,)*)> {
            type Item = ($($ty::Item,)*);

            unsafe fn fetch(&'w mut self, slot: usize) -> Self::Item {
                let inner = &mut self.0;
                ($(inner.$idx.fetch(slot), )*)
            }

            unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
                let inner = &mut self.0;

                [$(inner.$idx.filter_slots(slots)),*]
                    .into_iter()
                    .min()
                    .unwrap_or_default()
            }

            fn set_visited(&mut self, slots: crate::archetype::Slice) {
                let inner = &mut self.0;
                $(inner.$idx.set_visited(slots);)*
            }
        }

        impl<'q, $($ty: FetchItem<'q>,)*> FetchItem<'q> for Union<($($ty,)*)> {
            type Item = ($($ty::Item,)*);
        }

        // Or
        impl<'q, $($ty, )*> FetchItem<'q> for Or<($($ty,)*)> {
            type Item = ();
        }

        impl<'w, $($ty, )*> Fetch<'w> for Or<($($ty,)*)>
        where $($ty: Fetch<'w>,)*
        {
            const MUTABLE: bool =  $($ty::MUTABLE )|*;
            type Prepared       = Or<($(Option<$ty::Prepared>,)*)>;

            fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
                let inner = &self.0;
                Some( Or(($(inner.$idx.prepare(data),)*)) )
            }

            fn filter_arch(&self, arch: &Archetype) -> bool {
                let inner = &self.0;
                $(inner.$idx.filter_arch(arch))||*
            }

            fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
                 $(self.0.$idx.access(data, dst);)*
            }

            fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
                let mut s = f.debug_tuple("Or");
                    let inner = &self.0;
                $(
                    s.field(&FmtQuery(&inner.$idx));
                )*
                s.finish()
            }
        }


        impl<'q, $($ty, )*> PreparedFetch<'q> for Or<($(Option<$ty>,)*)>
        where $($ty: PreparedFetch<'q>,)*
        {
            type Item = ();

            unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
                let inner = &mut self.0;

                [
                    $( inner.$idx.filter_slots(slots)),*
                ]
                .into_iter()
                .min()
                .unwrap_or_default()

            }

            #[inline]
            unsafe fn fetch(&mut self, _: usize) -> Self::Item {}

            fn set_visited(&mut self, slots: Slice) {
                $( self.0.$idx.set_visited(slots);)*
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
