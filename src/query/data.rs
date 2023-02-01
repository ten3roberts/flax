use alloc::vec::Vec;
use atomic_refcell::AtomicRef;

use crate::{
    All, AsBorrow, Fetch, Planar, Query, QueryStrategy, SystemAccess, SystemContext, SystemData,
    World,
};

impl<Q, F, S> SystemAccess for Query<Q, F, S>
where
    Q: 'static + for<'x> Fetch<'x>,
    F: 'static + for<'x> Fetch<'x>,
    S: for<'x> QueryStrategy<'x, Q, F>,
{
    fn access(&self, world: &World) -> Vec<crate::system::Access> {
        self.strategy.access(world, &self.fetch)
    }
}

/// Provides a query and a borrow of the world during system execution
pub struct QueryData<'a, Q, F = All, S = Planar>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
{
    world: AtomicRef<'a, World>,
    query: &'a mut Query<Q, F, S>,
}

impl<'a, Q, F, S> SystemData<'a> for Query<Q, F, S>
where
    Q: 'static + for<'x> Fetch<'x>,
    F: 'static + for<'x> Fetch<'x>,
    S: 'static + for<'x> QueryStrategy<'x, Q, F>,
{
    type Value = QueryData<'a, Q, F, S>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        let world = ctx
            .world()
            .map_err(|_| eyre::eyre!(alloc::format!("Failed to borrow world for query")))?;

        Ok(QueryData { world, query: self })
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

impl<'a, 'w, Q, F, S> AsBorrow<'a> for QueryData<'w, Q, F, S>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Fetch<'x> + 'static,
    S: for<'x> QueryStrategy<'x, Q, F>,
    <S as QueryStrategy<'a, Q, F>>::Borrow: 'a,
{
    type Borrowed = <S as QueryStrategy<'a, Q, F>>::Borrow;

    fn as_borrow(&'a mut self) -> Self::Borrowed {
        self.borrow()
    }
}
