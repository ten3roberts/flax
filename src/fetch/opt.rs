use crate::{Fetch, PreparedFetch};

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct Opt<F>(pub(crate) F);

impl<F> Opt<F> {}

impl<'w, F> Fetch<'w> for Opt<F>
where
    F: for<'x> Fetch<'x>,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = PreparedOpt<<F as Fetch<'w>>::Prepared>;

    fn prepare(
        &'w self,
        world: &'w crate::World,
        archetype: &'w crate::Archetype,
    ) -> Option<Self::Prepared> {
        Some(PreparedOpt {
            inner: self.0.prepare(world, archetype),
        })
    }

    fn matches(&self, _: &'w crate::World, _: &'w crate::Archetype) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("opt {}", self.0.describe())
    }

    fn access(&self, id: crate::ArchetypeId, archetype: &crate::Archetype) -> Vec<crate::Access> {
        self.0.access(id, archetype)
    }

    fn difference(&self, _: &crate::Archetype) -> Vec<String> {
        vec![]
    }
}

#[doc(hidden)]
pub struct PreparedOpt<F> {
    inner: Option<F>,
}

impl<'q, F> PreparedFetch<'q> for PreparedOpt<F>
where
    F: for<'x> PreparedFetch<'x>,
{
    type Item = Option<<F as PreparedFetch<'q>>::Item>;

    unsafe fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
        self.inner.as_mut().map(|v| v.fetch(slot))
    }
}

/// Transform a fetch into a optional fetch
#[derive(Debug, Clone)]
pub struct OptOr<F, V> {
    inner: F,
    or: V,
}

impl<F, V> OptOr<F, V> {
    pub(crate) fn new(inner: F, or: V) -> Self {
        Self { inner, or }
    }
}

impl<'w, F, V> Fetch<'w> for OptOr<F, V>
where
    F: for<'x> Fetch<'x>,
    for<'x, 'y> <F as Fetch<'x>>::Prepared: PreparedFetch<'y, Item = &'y V>,
    V: 'static,
{
    const MUTABLE: bool = F::MUTABLE;

    type Prepared = PreparedOptOr<'w, <F as Fetch<'w>>::Prepared, V>;

    fn prepare(
        &'w self,
        world: &'w crate::World,
        archetype: &'w crate::Archetype,
    ) -> Option<Self::Prepared> {
        Some(PreparedOptOr {
            inner: self.inner.prepare(world, archetype),
            or: &self.or,
        })
    }

    fn matches(&self, _: &'w crate::World, _: &'w crate::Archetype) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("opt {}", self.inner.describe())
    }

    fn access(&self, id: crate::ArchetypeId, archetype: &crate::Archetype) -> Vec<crate::Access> {
        self.inner.access(id, archetype)
    }

    fn difference(&self, _: &crate::Archetype) -> Vec<String> {
        vec![]
    }
}

#[doc(hidden)]
pub struct PreparedOptOr<'w, F, V> {
    inner: Option<F>,
    or: &'w V,
}

impl<'q, 'w, F, V> PreparedFetch<'q> for PreparedOptOr<'w, F, V>
where
    F: for<'x> PreparedFetch<'x, Item = &'x V>,
    V: 'static,
{
    type Item = &'q V;

    unsafe fn fetch(&'q mut self, slot: crate::archetype::Slot) -> Self::Item {
        match self.inner {
            Some(ref mut v) => v.fetch(slot),
            None => self.or,
        }
    }
}
