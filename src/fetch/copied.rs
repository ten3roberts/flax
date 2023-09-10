use core::{
    fmt::{self, Formatter},
    ops::Deref,
};

use alloc::vec::Vec;

use crate::{archetype::Slice, system::Access, Fetch, FetchItem};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch, RandomFetch, TransformFetch};

#[derive(Debug, Clone)]
/// Component which copied the value.
///
/// This is useful as the query item is 'static
/// See [crate::Component::as_mut]
pub struct Copied<F>(pub(crate) F);

impl<'q, F> FetchItem<'q> for Copied<F>
where
    F: FetchItem<'q>,
    <F as FetchItem<'q>>::Item: Deref,
    <<F as FetchItem<'q>>::Item as Deref>::Target: 'static + Copy,
{
    type Item = <<F as FetchItem<'q>>::Item as Deref>::Target;
}

impl<'w, F> Fetch<'w> for Copied<F>
where
    F: Fetch<'w>,
    F: for<'q> FetchItem<'q>,
    for<'q> <F as FetchItem<'q>>::Item: Deref,
    for<'q> <<F as FetchItem<'q>>::Item as Deref>::Target: 'static + Copy,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = Copied<F::Prepared>;

    #[inline(always)]
    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(Copied(self.0.prepare(data)?))
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.0.filter_arch(data)
    }

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        self.0.access(data, dst)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("clone ")?;
        self.0.describe(f)
    }

    #[inline]
    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        self.0.searcher(searcher)
    }
}

impl<'q, F, V> PreparedFetch<'q> for Copied<F>
where
    F: PreparedFetch<'q>,
    F::Item: Deref<Target = V>,
    V: 'static + Copy,
{
    type Item = V;
    type Chunk = F::Chunk;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.0.create_chunk(slots)
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        *F::fetch_next(chunk)
    }

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        self.0.filter_slots(slots)
    }
}

impl<'q, F, V> RandomFetch<'q> for Copied<F>
where
    F: RandomFetch<'q>,
    F::Item: Deref<Target = V>,
    V: 'static + Copy,
{
    unsafe fn fetch_shared(&'q self, slot: crate::archetype::Slot) -> Self::Item {
        *self.0.fetch_shared(slot)
    }

    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: crate::archetype::Slot) -> Self::Item {
        *F::fetch_shared_chunk(chunk, slot)
    }
}

impl<K, F> TransformFetch<K> for Copied<F>
where
    F: TransformFetch<K>,
    Copied<F>: for<'x> Fetch<'x>,
    Copied<F::Output>: for<'x> Fetch<'x>,
{
    type Output = Copied<F::Output>;

    fn transform_fetch(self, method: K) -> Self::Output {
        Copied(self.0.transform_fetch(method))
    }
}
