use crate::{fetch::PreparedFetch, Fetch};

use super::opt::{Opt, OptOr};

/// Extension trait for [crate::Fetch]
pub trait FetchExt: Sized {
    /// Transform the fetch into an optional fetch, yielding Some or None
    fn opt(self) -> Opt<Self> {
        Opt(self)
    }

    /// Transform the fetch into a fetch with a provided default.
    /// This is useful for default values such as scale or velocity which may
    /// not exist for every entity.
    fn opt_or<V>(self, default: V) -> OptOr<Self, V>
    where
        Self: for<'x> Fetch<'x>,
        for<'x, 'y> <Self as Fetch<'x>>::Prepared: PreparedFetch<'y, Item = &'y V>,
    {
        OptOr::new(self, default)
    }

    /// Transform the fetch into a fetch which yields the default impl if the
    /// fetch is not matched.
    fn opt_or_default<V>(self) -> OptOr<Self, V>
    where
        Self: for<'x> Fetch<'x>,
        for<'x, 'y> <Self as Fetch<'x>>::Prepared: PreparedFetch<'y, Item = &'y V>,
        V: Default + 'static,
    {
        self.opt_or(Default::default())
    }
}

impl<F> FetchExt for F where F: for<'x> Fetch<'x> {}
