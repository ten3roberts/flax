use alloc::vec::Vec;
use core::{iter::Flatten, slice::IterMut};
use smallvec::SmallVec;

use crate::{
    archetype::Slice,
    entity::EntityLocation,
    error::Result,
    fetch::{FetchAccessData, PreparedFetch},
    filter::Filtered,
    Access, AccessKind, All, ArchetypeChunks, ArchetypeId, Batch, Entity, Error, Fetch, FetchItem,
    PreparedArchetype, QueryStrategy, World,
};

use super::{borrow::QueryBorrowState, difference::find_missing_components, ArchetypeSearcher};

/// The default linear iteration strategy
#[derive(Clone)]
pub struct Planar {
    pub(super) archetypes: Vec<ArchetypeId>,
}

impl core::fmt::Debug for Planar {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Planar").finish()
    }
}

impl Planar {
    pub(super) fn new() -> Self {
        Self {
            archetypes: Vec::new(),
        }
    }
}

impl Planar {
    // Make sure the archetypes to visit are up to date
    fn update_state<'w, Q: Fetch<'w>, F: Fetch<'w>>(
        world: &crate::World,
        fetch: &Filtered<Q, F>,
        result: &mut Vec<ArchetypeId>,
    ) {
        result.clear();

        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);

        searcher.find_archetypes(&world.archetypes, |arch_id, arch| {
            if !fetch.filter_arch(arch) {
                return;
            }

            result.push(arch_id)
        });

        // let mut unique = BTreeSet::new();

        // assert!(
        //     result.iter().all(|v| unique.insert(v)),
        //     "Duplicate archetypes: {result:?}"
        // );
    }
}

impl<'w, Q, F> QueryStrategy<'w, Q, F> for Planar
where
    Q: 'w + Fetch<'w>,
    F: 'w + Fetch<'w>,
{
    type Borrow = QueryBorrow<'w, Q, F>;

    fn borrow(&'w mut self, state: QueryBorrowState<'w, Q, F>, dirty: bool) -> Self::Borrow {
        // Make sure the archetypes to visit are up to date
        if dirty {
            Self::update_state(state.world, state.fetch, &mut self.archetypes);
        }

        QueryBorrow {
            prepared: SmallVec::new(),
            archetypes: &self.archetypes,
            state,
        }
    }

    fn access(&self, world: &World, fetch: &Filtered<Q, F>) -> Vec<crate::Access> {
        let mut result = Vec::new();
        Self::update_state(world, fetch, &mut result);

        result
            .iter()
            .flat_map(|&arch_id| {
                let arch = world.archetypes.get(arch_id);
                let data = FetchAccessData {
                    world,
                    arch,
                    arch_id,
                };

                fetch.access(data)
            })
            .chain([Access {
                kind: AccessKind::World,
                mutable: false,
            }])
            .collect()
    }
}

/// A lazily prepared query which borrows and hands out chunk iterators for
/// each archetype matched.
///
/// The borrowing is lazy, as such, calling [`QueryBorrow::get`] will only borrow the one required archetype.
/// [`QueryBorrow::iter`] will borrow the components from all archetypes and release them once the prepared query drops.
/// Subsequent calls to iter will use the same borrow.
pub struct QueryBorrow<'w, Q, F = All>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 8]>,
    archetypes: &'w [ArchetypeId],
    state: QueryBorrowState<'w, Q, F>,
}

impl<'w, 'q, Q, F> IntoIterator for &'q mut QueryBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    type IntoIter = QueryIter<'w, 'q, Q, F>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'w, Q, F> QueryBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Iterate all items matched by query and filter.
    #[inline]
    pub fn iter<'q>(&'q mut self) -> QueryIter<'w, 'q, Q, F>
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
    pub fn iter_batched<'q>(&'q mut self) -> BatchedIter<'w, 'q, Q, F>
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

                    self.state.prepare_fetch(arch_id, arch)
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

            let Some(mut p) = self.state.prepare_fetch(arch_id, arch) else { continue };

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
        F: Sync,
        F::Prepared: Send,
        // BatchedIter<'q, 'w, Q>: Send,
    {
        use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

        self.clear_borrows();
        self.archetypes.par_iter().for_each(|&arch_id| {
            let arch = self.state.world.archetypes.get(arch_id);
            if arch.is_empty() {
                return;
            }

            let Some(mut p) = self.state.prepare_fetch(arch_id, arch) else { return };

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

            if !self.state.fetch.filter_arch(arch) {
                return None;
            }

            let fetch = self.state.prepare_fetch(arch_id, arch)?;

            // let arch_id = *self.archetypes.iter().find(|&&v| v == arch_id)?;

            prepared.push(fetch);

            Some(prepared.len() - 1)
        }
    }

    /// Get the fetch items for an entity.
    pub fn get(&mut self, id: Entity) -> Result<<Q::Prepared as PreparedFetch>::Item> {
        let EntityLocation { arch_id, slot } = self.state.world.location(id)?;

        let idx =
            self.prepare_archetype(arch_id).ok_or_else(|| {
                match find_missing_components(self.state.fetch, arch_id, self.state.world).next() {
                    Some(missing) => Error::MissingComponent(id, missing),
                    None => Error::DoesNotMatch(id),
                }
            })?;

        // Since `self` is a mutable references the borrow checker
        // guarantees this borrow is unique
        let p = &mut self.prepared[idx];
        let mut chunk = p
            .manual_chunk(Slice::single(slot))
            .ok_or(Error::Filtered(id))?;

        let item = chunk.next().unwrap();

        Ok(item)
    }

    pub(crate) fn archetypes(&self) -> &[ArchetypeId] {
        self.archetypes
    }
}

/// The query iterator
pub struct QueryIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    iter: Flatten<BatchedIter<'w, 'q, Q, F>>,
}

impl<'w, 'q, Q, F> Iterator for QueryIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
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
pub struct BatchedIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    pub(crate) archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared, F::Prepared>>,
    pub(crate) current: Option<ArchetypeChunks<'q, Q::Prepared, F::Prepared>>,
}

impl<'q, 'w, Q, F> BatchedIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    pub(crate) fn new(
        archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared, F::Prepared>>,
    ) -> Self {
        Self {
            archetypes,
            current: None,
        }
    }
}

impl<'w, 'q, Q, F> Iterator for BatchedIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    type Item = Batch<'q, Q::Prepared, F::Prepared>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(chunk) = self.current.as_mut() {
                if let item @ Some(..) = chunk.next() {
                    return item;
                }
            }

            let p = self.archetypes.next()?;
            self.current = Some(p.chunks());
        }
    }
}
