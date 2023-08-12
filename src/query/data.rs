use alloc::vec::Vec;
use atomic_refcell::AtomicRef;

use crate::{
    filter::All,
    system::{Access, AsBorrowed, SystemAccess, SystemContext, SystemData},
    Fetch, Planar, Query, World,
};

use super::QueryStrategy;

impl<Q, F, S> SystemAccess for Query<Q, F, S>
where
    Q: 'static + for<'x> Fetch<'x>,
    F: 'static + for<'x> Fetch<'x>,
    S: for<'x> QueryStrategy<'x, Q, F>,
{
    fn access(&self, world: &World, dst: &mut Vec<Access>) {
        self.strategy.access(world, &self.fetch, dst);
    }
}

/// Combined reference to a query and a world.
///
/// Allow for executing a query inside a system without violating access rules.
pub struct QueryData<'a, Q, F = All, S = Planar>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    world: AtomicRef<'a, World>,
    query: &'a mut Query<Q, F, S>,
}

impl<'a, Q, F, S, T> SystemData<'a, T> for Query<Q, F, S>
where
    Q: 'static + for<'x> Fetch<'x>,
    F: 'static + for<'x> Fetch<'x>,
    S: 'static + for<'x> QueryStrategy<'x, Q, F>,
{
    type Value = QueryData<'a, Q, F, S>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_, T>) -> Self::Value {
        let world = ctx.world();

        QueryData { world, query: self }
    }

    fn describe(&self, f: &mut alloc::fmt::Formatter<'_>) -> alloc::fmt::Result {
        f.write_str("Query<")?;
        self.fetch.describe(f)?;
        f.write_str(", ")?;
        f.write_str(&tynm::type_name::<S>())?;
        f.write_str(">")
    }
}

impl<'a, Q, F, S> QueryData<'a, Q, F, S>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
    S: for<'x> QueryStrategy<'x, Q, F>,
{
    /// Prepare the query.
    ///
    /// This will borrow all required archetypes for the duration of the
    /// `PreparedQuery`.
    ///
    /// The same query can be prepared multiple times, though not
    /// simultaneously.
    pub fn borrow(&mut self) -> <S as QueryStrategy<Q, F>>::Borrow {
        self.query.borrow(&self.world)
    }
}

impl<'a, 'w, Q, F, S> AsBorrowed<'a> for QueryData<'w, Q, F, S>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
    S: for<'x> QueryStrategy<'x, Q, F>,
    <S as QueryStrategy<'a, Q, F>>::Borrow: 'a,
{
    type Borrowed = <S as QueryStrategy<'a, Q, F>>::Borrow;

    fn as_borrowed(&'a mut self) -> Self::Borrowed {
        self.borrow()
    }
}
