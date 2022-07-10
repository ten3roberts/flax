use crate::{Component, ComponentValue, Fetch, PreparedFetch};

use super::opt::{Opt, OptOr};

pub trait FetchExt: Sized {
    /// Transform the query into an optional query, yielding Some or None
    fn opt(self) -> Opt<Self> {
        Opt::new(self)
    }

    /// Transform the query into a query with a provided default.
    /// This is useful for default values such as scale or velocity which may
    /// not exist for every entity.
    fn opt_or<V>(self, default: V) -> OptOr<Self, V>
    where
        Self: for<'x> Fetch<'x>,
        for<'x, 'y> <Self as Fetch<'x>>::Prepared: PreparedFetch<'y, Item = &'y V>,
    {
        OptOr::new(self, default)
    }

    fn opt_or_default<V>(self) -> OptOr<Self, V>
    where
        Self: for<'x> Fetch<'x>,
        for<'x, 'y> <Self as Fetch<'x>>::Prepared: PreparedFetch<'y, Item = &'y V>,
        V: Default + 'static,
    {
        self.opt_or(Default::default())
    }
}

impl<T> FetchExt for Component<T> where T: ComponentValue {}
