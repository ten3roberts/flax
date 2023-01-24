use core::{
    iter::Flatten,
    mem::{self, MaybeUninit},
    slice::IterMut,
};

use smallvec::SmallVec;

use crate::{
    archetype::{unknown_component, Slice},
    dummy,
    entity::EntityLocation,
    error::Result,
    fetch::PreparedFetch,
    query::ArchetypeSearcher,
    ArchetypeId, Entity, Error, Fetch, FetchItem,
};

use super::{
    borrow::{PreparedArchetype, QueryBorrowState},
    difference::find_missing_components,
    iter::{ArchetypeChunks, Batch},
    QueryState, QueryStrategy,
};

/// The default linear iteration strategy
pub struct Planar;

impl<Q: 'static + for<'x> Fetch<'x>> QueryStrategy<Q> for Planar {
    type State = PlanarState;

    fn state(&self, world: &crate::World, fetch: &Q) -> Self::State {
        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);

        let mut archetypes = Vec::new();
        searcher.find_archetypes(&world.archetypes, |arch_id, arch| {
            if !fetch.filter_arch(arch) {
                return;
            }
            archetypes.push(arch_id)
        });

        PlanarState { archetypes }
    }
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct PlanarState {
    archetypes: Vec<ArchetypeId>,
}

impl<'w, Q> QueryState<'w, Q> for PlanarState
where
    Q: 'w + Fetch<'w>,
{
    type Borrow = QueryBorrow<'w, Q>;

    fn borrow(&'w self, query_state: super::borrow::QueryBorrowState<'w, Q>) -> Self::Borrow {
        QueryBorrow {
            prepared: SmallVec::new(),
            archetypes: &self.archetypes,
            state: query_state,
        }
    }
}

/// A lazily prepared query which borrows and hands out chunk iterators for
/// each archetype matched.
///
/// The borrowing is lazy, as such, calling [`QueryBorrow::get`] will only borrow the one required archetype.
/// [`QueryBorrow::iter`] will borrow the components from all archetypes and release them once the prepared query drops.
/// Subsequent calls to iter will use the same borrow.
pub struct QueryBorrow<'w, Q>
where
    Q: Fetch<'w>,
{
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared>; 8]>,
    archetypes: &'w [ArchetypeId],
    state: QueryBorrowState<'w, Q>,
}

impl<'w, 'q, Q> IntoIterator for &'q mut QueryBorrow<'w, Q>
where
    Q: Fetch<'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    type IntoIter = QueryIter<'q, 'w, Q>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'w, Q> QueryBorrow<'w, Q>
where
    Q: Fetch<'w>,
{
    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> QueryIter<'q, 'w, Q>
    where
        'w: 'q,
    {
        QueryIter {
            iter: self.iter_batched().flatten(),
        }
    }

    /// Returns the first item
    pub fn first(&mut self) -> Option<<Q as FetchItem<'_>>::Item> {
        self.iter().next()
    }

    /// Iterate all items matched by query and filter.
    pub fn iter_batched<'q>(&'q mut self) -> BatchedIter<'q, 'w, Q>
    where
        'w: 'q,
    {
        // Prepare all archetypes only if it is not already done
        // Clear previous borrows
        if self.prepared.len() != self.archetypes.len() {
            self.clear_borrows();
            self.prepared = self
                .archetypes
                .iter()
                .filter_map(|&arch_id| {
                    let arch = self.state.world.archetypes.get(arch_id);
                    if arch.is_empty() {
                        return None;
                    }

                    self.state.prepare_fetch(arch, arch_id)
                })
                .collect();
        }

        BatchedIter {
            archetypes: self.prepared.iter_mut(),
            current: None,
        }
    }

    /// Execute a closure for each item in the iterator.
    ///
    /// This is more efficient than `.iter().for_each(|v| {})` as the archetypes can be temporarily
    /// borrowed.
    pub fn for_each(&mut self, func: impl Fn(<Q as FetchItem<'_>>::Item) + Send + Sync) {
        self.clear_borrows();
        for &arch_id in self.archetypes {
            let arch = self.state.world.archetypes.get(arch_id);
            if arch.is_empty() {
                continue;
            }

            let Some(mut p) = self.state.prepare_fetch(arch, arch_id) else { continue };

            let chunk = p.chunks();

            for item in chunk.flatten() {
                func(item)
            }
        }
    }

    /// Shorthand for:
    /// ```rust,ignore
    /// self.iter_batched()
    ///     .par_bridge()
    ///     .for_each(|v| v.for_each(&func))
    /// ```
    #[cfg(feature = "parallel")]
    pub fn par_for_each(&mut self, func: impl Fn(<Q as FetchItem<'_>>::Item) + Send + Sync)
    where
        Q: Sync,
        Q::Prepared: Send,
        // BatchedIter<'q, 'w, Q>: Send,
    {
        use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

        self.clear_borrows();
        self.archetypes.par_iter().for_each(|&arch_id| {
            let arch = self.state.world.archetypes.get(arch_id);
            if arch.is_empty() {
                return;
            }

            let Some(mut p) = self.state.prepare_fetch(arch, arch_id) else { return };

            let chunk = p.chunks();

            for item in chunk.flatten() {
                func(item)
            }
        });
    }

    /// Release all borrowed archetypes
    pub fn clear_borrows(&mut self) {
        self.prepared.clear()
    }

    /// Consumes the iterator and returns the number of entities visited.
    /// Faster than `self.iter().count()`
    pub fn count<'q>(&'q mut self) -> usize
    where
        'w: 'q,
    {
        self.iter_batched().map(|v| v.slots().len()).sum()
    }

    fn prepare_archetype(&mut self, arch_id: ArchetypeId) -> Option<usize> {
        let prepared = &mut self.prepared;

        if let Some(idx) = prepared.iter().position(|v| v.arch_id == arch_id) {
            Some(idx)
        } else {
            let arch = self.state.world.archetypes.get(arch_id);

            if self.state.fetch.filter_arch(arch) {
                return None;
            }

            let fetch = self.state.prepare_fetch(arch, arch_id)?;

            // let arch_id = *self.archetypes.iter().find(|&&v| v == arch_id)?;

            prepared.push(fetch);

            Some(prepared.len() - 1)
        }
    }

    /// Access any number of entites which are disjoint.
    /// Return None if any `id` is not disjoint.
    /// See: [`Self::get`]
    pub fn get_disjoint<'q, const C: usize>(
        &'q mut self,
        ids: [Entity; C],
    ) -> Result<[<Q::Prepared as PreparedFetch>::Item; C]>
    where
        'w: 'q,
    {
        let mut sorted = ids;
        sorted.sort();
        if sorted.windows(C).any(|v| v[0] == v[1]) {
            // Not disjoint
            return Err(Error::Disjoint(ids.to_vec()));
        }

        // Prepare all
        let mut idxs = [(0, 0, dummy()); C];

        for i in 0..C {
            let id = ids[i];
            let EntityLocation { arch_id, slot } = self.state.world.location(id)?;
            let idx = self.prepare_archetype(arch_id).ok_or_else(|| {
                Error::MissingComponent(
                    id,
                    find_missing_components(self.state.fetch, arch_id, self.state.world)
                        .next()
                        .unwrap_or_else(|| unknown_component().info()),
                )
            })?;

            idxs[i] = (idx, slot, id);
        }

        // Fetch all
        // All items will be initialized
        let mut items: [MaybeUninit<_>; C] = unsafe { MaybeUninit::uninit().assume_init() };

        for i in 0..C {
            let (idx, slot, id) = idxs[i];

            let prepared = &mut self.prepared[idx];

            // All entities are disjoint at this point
            let prepared = unsafe { &mut *(prepared as *mut PreparedArchetype<_>) };

            let mut chunk = match prepared.manual_chunk(Slice::single(slot)) {
                Some(v) => v,
                None => return Err(Error::MismatchedFilter(id)),
            };

            items[i].write(chunk.next().unwrap());
        }

        unsafe {
            let items = mem::transmute_copy::<_, [<Q::Prepared as PreparedFetch>::Item; C]>(&items);
            Ok(items)
        }
    }

    /// Get the fetch items for an entity.
    pub fn get(&mut self, id: Entity) -> Result<<Q::Prepared as PreparedFetch>::Item> {
        let EntityLocation { arch_id, slot } = self.state.world.location(id)?;

        let idx = self.prepare_archetype(arch_id).ok_or_else(|| {
            Error::MissingComponent(
                id,
                find_missing_components(self.state.fetch, arch_id, self.state.world)
                    .next()
                    .unwrap_or_else(|| unknown_component().info()),
            )
        })?;

        // Since `self` is a mutable references the borrow checker
        // guarantees this borrow is unique
        let p = &mut self.prepared[idx];
        let mut chunk = match p.manual_chunk(Slice::single(slot)) {
            Some(v) => v,
            None => return Err(Error::MismatchedFilter(id)),
        };

        let item = chunk.next().unwrap();

        Ok(item)
    }
}

/// The query iterator
pub struct QueryIter<'q, 'w, Q>
where
    Q: Fetch<'w>,
{
    iter: Flatten<BatchedIter<'q, 'w, Q>>,
}

impl<'w, 'q, Q> Iterator for QueryIter<'q, 'w, Q>
where
    Q: Fetch<'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

/// An iterator which yields disjoint continuous slices for each matched archetype
/// and filter predicate.
pub struct BatchedIter<'q, 'w, Q>
where
    Q: Fetch<'w>,
    'w: 'q,
{
    pub(crate) archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared>>,
    pub(crate) current: Option<ArchetypeChunks<'q, Q::Prepared>>,
}

impl<'w, 'q, Q> Iterator for BatchedIter<'q, 'w, Q>
where
    Q: Fetch<'w>,
    'w: 'q,
{
    type Item = Batch<'q, Q::Prepared>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(chunk) = self.current.as_mut() {
                if let item @ Some(..) = chunk.next() {
                    return item;
                }
            }

            let PreparedArchetype { arch, fetch, .. } = self.archetypes.next()?;

            let chunk = ArchetypeChunks {
                slots: arch.slots(),
                arch,
                fetch,
            };

            self.current = Some(chunk);
        }
    }
}
