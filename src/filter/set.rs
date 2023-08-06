use crate::{
    archetype::{Archetype, Slice, Slot},
    fetch::{FetchAccessData, FetchPrepareData, FmtQuery, PreparedFetch, UnionFilter},
    system::Access,
    Fetch, FetchItem,
};
use alloc::vec::Vec;
use core::{
    fmt::{self, Formatter},
    ops,
};

/// And combinator
///
/// **Note**: A normal tuple will and-combine and can thus be used instead.
///
/// The difference is that additional *bitops* such as `|` and `~` for convenience works on this type
/// to combine it with other filters. This is because of orphan rules.
#[derive(Debug, Clone)]
pub struct And<L, R>(pub L, pub R);

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
        Some(And(self.0.prepare(data)?, self.1.prepare(data)?))
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        self.0.filter_arch(arch) && self.1.filter_arch(arch)
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.0.access(data, dst);
        self.1.access(data, dst);
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.describe(f)?;
        f.write_str(" & ")?;
        self.1.describe(f)?;

        Ok(())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        self.0.searcher(searcher);
        self.1.searcher(searcher);
    }
}

impl<'q, L, R> PreparedFetch<'q> for And<L, R>
where
    L: PreparedFetch<'q>,
    R: PreparedFetch<'q>,
{
    type Item = (L::Item, R::Item);

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        let l = self.0.filter_slots(slots);

        self.1.filter_slots(l)
    }

    type Chunk = (L::Chunk, R::Chunk);

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        (self.0.create_chunk(slots), self.1.create_chunk(slots))
    }

    #[inline]
    unsafe fn fetch_next(chunk: &mut Self::Chunk, slot: Slot) -> Self::Item {
        (
            L::fetch_next(&mut chunk.0, slot),
            R::fetch_next(&mut chunk.1, slot),
        )
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

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        if let Some(fetch) = &mut self.0 {
            let v = fetch.filter_slots(slots);

            slots.difference(v).unwrap()
        } else {
            slots
        }
    }

    type Chunk = ();

    #[inline]
    unsafe fn create_chunk(&'q mut self, _: Slice) -> Self::Chunk {}

    #[inline]
    unsafe fn fetch_next(_: &mut Self::Chunk, _: Slot) -> Self::Item {}
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
///
/// For this to implement `Fetch`, `T::Prepared` must implement `UnionFilter`.
#[derive(Debug, Clone)]
pub struct Union<T>(pub T);

impl<'q, T> FetchItem<'q> for Union<T>
where
    T: FetchItem<'q>,
{
    type Item = T::Item;
}

impl<'w, T> Fetch<'w> for Union<T>
where
    T: Fetch<'w>,
    T::Prepared: UnionFilter,
{
    const MUTABLE: bool = T::MUTABLE;

    type Prepared = Union<T::Prepared>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(Union(self.0.prepare(data)?))
    }

    fn filter_arch(&self, arch: &Archetype) -> bool {
        self.0.filter_arch(arch)
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.0.access(data, dst)
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Union").field(&FmtQuery(&self.0)).finish()
    }
}

impl<T> UnionFilter for Union<T>
where
    T: UnionFilter,
{
    unsafe fn filter_union(&mut self, slots: Slice) -> Slice {
        self.0.filter_union(slots)
    }
}

impl<'q, T> PreparedFetch<'q> for Union<T>
where
    T: PreparedFetch<'q> + UnionFilter,
{
    type Item = T::Item;

    #[inline]
    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        self.filter_union(slots)
    }

    type Chunk = T::Chunk;

    #[inline]
    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.0.create_chunk(slots)
    }

    #[inline]
    unsafe fn fetch_next(chunk: &mut Self::Chunk, slot: Slot) -> Self::Item {
        T::fetch_next(chunk, slot)
    }
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        // Or
        impl<'w, 'q, $($ty, )*> FetchItem<'q> for Or<($($ty,)*)> {
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


        impl<'w, 'q, $($ty, )*> PreparedFetch<'q> for Or<($(Option<$ty>,)*)>
        where 'w: 'q, $($ty: PreparedFetch<'q>,)*
        {
            type Item = ();
            type Chunk = ();

            unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
                let inner = &mut self.0;
                let end = Slice::new(slots.end, slots.end);

                [
                    $( inner.$idx.as_mut().map(|v| v.filter_slots(slots)).unwrap_or(end)),*
                ]
                .into_iter()
                .min()
                .unwrap_or_default()

            }

            #[inline]
            unsafe fn fetch_next(_: &mut Self::Chunk, _:Slot) -> Self::Item {}

            #[inline]
            unsafe fn create_chunk(&mut self, _: Slice) -> Self::Chunk {}

        }

        impl<'q, $($ty, )*> UnionFilter for Or<($(Option<$ty>,)*)>
        where $($ty: PreparedFetch<'q>,)*
        {
            unsafe fn filter_union(&mut self, slots: Slice) -> Slice {
                let inner = &mut self.0;
                let end = Slice::new(slots.end, slots.end);

                [
                    $( inner.$idx.as_mut().map(|v| v.filter_slots(slots)).unwrap_or(end)),*
                ]
                .into_iter()
                .min()
                .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use crate::filter::{FilterIter, Nothing};

    use super::*;

    #[test]
    fn union() {
        let fetch = Union((
            Slice::new(0, 2),
            Nothing,
            Slice::new(7, 16),
            Slice::new(3, 10),
        ));

        let fetch = FilterIter::new(Slice::new(0, 100), fetch);

        assert_eq!(
            fetch.collect_vec(),
            [Slice::new(0, 2), Slice::new(3, 10), Slice::new(10, 16)]
        );
    }
}
