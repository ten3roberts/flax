use core::fmt::{self, Formatter};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slot},
    filter::Nothing,
    Access, ComponentKey, ComponentValue, Fetch, FetchItem,
};

use super::{FetchPrepareData, PreparedFetch};

#[doc(hidden)]
pub struct PreparedCloned<F> {
    inner: F,
}

#[derive(Debug, Clone)]
/// Component which clones the value.
///
/// This is useful as the query item is 'static
/// See [crate::Component::as_mut]
pub struct Cloned<F>(pub(crate) F);

impl<'w, F, V> Fetch<'w> for Cloned<F>
where
    F: Fetch<'w> + for<'q> FetchItem<'q, Item = &'q V>,
    for<'q> <F as Fetch<'w>>::Prepared: PreparedFetch<'q, Item = &'q V>,
    V: ComponentValue + Clone,
{
    const MUTABLE: bool = false;
    const HAS_FILTER: bool = false;

    type Filter = Nothing;

    type Prepared = PreparedCloned<F::Prepared>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let inner = self.0.prepare(data)?;

        Some(PreparedCloned { inner })
    }

    fn matches(&self, arch: &Archetype) -> bool {
        self.0.matches(arch)
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        self.0.access(data)
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("clone ")?;
        self.0.describe(f)
    }

    fn filter(&self) -> Self::Filter {
        Nothing
    }

    fn components(&self, result: &mut Vec<ComponentKey>) {
        self.0.components(result)
    }
}

impl<'q, F, V> FetchItem<'q> for Cloned<F>
where
    F: FetchItem<'q, Item = &'q V>,
    V: ComponentValue + Clone,
{
    type Item = V;
}

impl<'q, F, V> PreparedFetch<'q> for PreparedCloned<F>
where
    F: PreparedFetch<'q, Item = &'q V>,
    V: 'q + Clone,
{
    type Item = V;

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        self.inner.fetch(slot).clone()
    }
}
