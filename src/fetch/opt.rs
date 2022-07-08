// use crate::{Fetch, PreparedFetch};

// /// Transform a fetch into a optional fetch
// pub struct Opt<F> {
//     inner: F,
// }

// impl<'w, F> Fetch<'w> for Opt<F>
// where
//     F: for<'x> Fetch<'x>,
// {
//     const MUTABLE: bool = F::MUTABLE;

//     type Prepared = PreparedOpt<<F as Fetch<'w>>::Prepared>;

//     fn prepare(
//         &self,
//         world: &'w crate::World,
//         archetype: &'w crate::Archetype,
//     ) -> Option<Self::Prepared> {
//         Some(PreparedOpt {
//             inner: self.inner.prepare(world, archetype),
//         })
//     }

//     fn matches(&self, _: &'w crate::World, _: &'w crate::Archetype) -> bool {
//         true
//     }

//     fn describe(&self) -> String {
//         format!("opt {}", self.inner.describe())
//     }

//     fn access(&self, id: crate::ArchetypeId, archetype: &crate::Archetype) -> Vec<crate::Access> {
//         self.inner.access(id, archetype)
//     }

//     fn difference(&self, _: &crate::Archetype) -> Vec<String> {
//         vec![]
//     }
// }

// #[doc(hidden)]
// pub struct PreparedOpt<F> {
//     inner: Option<F>,
// }

// impl<'q, F> PreparedFetch<'q> for PreparedOpt<F>
// where
//     F: for<'x> PreparedFetch<'x>,
// {
//     type Item = Option<<F as PreparedFetch<'q>>::Item>;

//     unsafe fn fetch(&'q self, slot: crate::archetype::Slot) -> Self::Item {
//         match self.inner {
//             Some(ref v) => Some(v.fetch(slot)),
//             None => None,
//         }
//     }
// }

// /// Transform a fetch into a optional fetch
// pub struct OptOr<F, V> {
//     inner: F,
//     or: V,
// }

// impl<'w, F, V> Fetch<'w> for OptOr<F, V>
// where
//     F: for<'x> Fetch<'x>,
//     for<'x> <F as Fetch<'x>>::Prepared: PreparedFetch<'x, Item = &'x V>,
//     V: 'static,
// {
//     const MUTABLE: bool = F::MUTABLE;

//     type Prepared = PreparedOptOr<'w, <F as Fetch<'w>>::Prepared, V>;

//     fn prepare(
//         &self,
//         world: &'w crate::World,
//         archetype: &'w crate::Archetype,
//     ) -> Option<Self::Prepared> {
//         Some(PreparedOptOr {
//             inner: self.inner.prepare(world, archetype),
//             or: &self.or,
//         })
//     }

//     fn matches(&self, _: &'w crate::World, _: &'w crate::Archetype) -> bool {
//         true
//     }

//     fn describe(&self) -> String {
//         format!("opt {}", self.inner.describe())
//     }

//     fn access(&self, id: crate::ArchetypeId, archetype: &crate::Archetype) -> Vec<crate::Access> {
//         self.inner.access(id, archetype)
//     }

//     fn difference(&self, _: &crate::Archetype) -> Vec<String> {
//         vec![]
//     }
// }

// #[doc(hidden)]
// pub struct PreparedOptOr<'q, F, V> {
//     inner: Option<F>,
//     or: &'q V,
// }

// impl<'q, F, V> PreparedFetch<'q> for PreparedOptOr<'q, F, V>
// where
//     F: for<'x> PreparedFetch<'x, Item = &'q V>,
// {
//     type Item = <F as PreparedFetch<'q>>::Item;

//     unsafe fn fetch(&'q self, slot: crate::archetype::Slot) -> Self::Item {
//         match self.inner {
//             Some(ref v) => v.fetch(slot),
//             None => self.or,
//         }
//     }
// }
