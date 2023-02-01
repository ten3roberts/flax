use crate::{Fetch, FetchItem};

use super::FmtQuery;

/// Yields true iff `F` would match the query
pub struct Satisfied<F>(pub(crate) F);

impl<'q, F: FetchItem<'q>> FetchItem<'q> for Satisfied<F> {
    type Item = bool;
}

impl<'w, F: Fetch<'w>> Fetch<'w> for Satisfied<F> {
    const MUTABLE: bool = false;

    type Prepared = bool;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Self::Prepared {
        self.0.filter_arch(data.arch)
    }

    fn filter_arch(&self, _: &crate::archetype::Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "satisfied {:?}", FmtQuery(&self.0))
    }
}
