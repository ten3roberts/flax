use core::{
    iter::Peekable,
    mem::{self, MaybeUninit},
};

#[cfg(feature = "parallel")]
use rayon::prelude::{ParallelBridge, ParallelIterator};
use smallvec::SmallVec;

use crate::{
    archetype::unknown_component,
    filter::{FilterIter, PreparedFilter, RefFilter},
    Chunks,
};
use crate::{
    component_info,
    entity::EntityLocation,
    error::Result,
    fetch::FetchPrepareData,
    fetch::PreparedFetch,
    filter::All,
    filter::{And, GatedFilter},
    Archetype, ArchetypeId, Entity, Error, Fetch, FetchItem, Filter, World,
};
use crate::{dummy, ComponentInfo};

use super::{
    iter::{BatchedIter, QueryIter},
    FilterWithFetch,
};

pub(crate) fn find_missing_components<'q, 'a, Q>(
    fetch: &Q,
    arch_id: ArchetypeId,
    world: &'a World,
) -> impl Iterator<Item = ComponentInfo> + 'a
where
    Q: Fetch<'a>,
{
    let arch = world.archetypes.get(arch_id);

    let mut searcher = Default::default();
    fetch.searcher(&mut searcher);
    DifferenceIter::new(
        arch.components().map(|v| v.key()),
        searcher.required.into_iter(),
    )
    .map(|v| *world.get(v.id, component_info()).unwrap())
}

pub(crate) struct PreparedArchetype<'w, Q> {
    pub(crate) arch_id: ArchetypeId,
    pub(crate) arch: &'w Archetype,
    pub(crate) fetch: Q,
}

/// A lazily prepared query which borrows and hands out chunk iterators for
/// each archetype matched.
pub struct QueryBorrow<'w, Q, F = All>
where
    Q: Fetch<'w>,
{
    pub(crate) prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared>; 8]>,
    pub(crate) world: &'w World,
    pub(crate) archetypes: &'w [ArchetypeId],
    pub(crate) fetch: &'w Q,
    pub(crate) filter: FilterWithFetch<RefFilter<'w, F>, Q::Filter>,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
}

impl<'w, 'q, Q, F> IntoIterator for &'q mut QueryBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q>,
    'w: 'q,
{
    type Item = <Q as FetchItem<'q>>::Item;

    type IntoIter = QueryIter<'q, 'w, Q, F>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

struct DifferenceIter<T, L: Iterator<Item = T>, R: Iterator<Item = T>> {
    left: Peekable<L>,
    right: Peekable<R>,
}

impl<T, L: Iterator<Item = T>, R: Iterator<Item = T>> DifferenceIter<T, L, R> {
    fn new(left: L, right: R) -> Self {
        Self {
            left: left.peekable(),
            right: right.peekable(),
        }
    }
}

impl<T: Ord, L: Iterator<Item = T>, R: Iterator<Item = T>> Iterator for DifferenceIter<T, L, R> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (l, r) = match (self.left.peek(), self.right.peek()) {
                (None, None) => return None,
                (None, Some(_)) => return self.right.next(),
                (Some(_), None) => return self.left.next(),
                (Some(l), Some(r)) => (l, r),
            };

            match l.cmp(r) {
                core::cmp::Ordering::Less => return self.left.next(),
                core::cmp::Ordering::Equal => {
                    self.left.next();
                    self.right.next();
                }
                core::cmp::Ordering::Greater => return self.right.next(),
            }
        }
    }
}
#[cfg(test)]
mod test {
    use itertools::Itertools;

    use super::DifferenceIter;

    #[test]
    fn difference_iter() {
        let diff = DifferenceIter::new([1, 2, 6, 7].into_iter(), [1, 2, 4, 5, 6, 8].into_iter())
            .collect_vec();
        assert_eq!(diff, [4, 5, 7, 8]);
    }
}

/// Represents a query that is bounded to the lifetime of the world.
/// Contains the borrows and holds them until it is dropped.
///
/// The borrowing is lazy, as such, calling [`QueryBorrow::get`] will only borrow the one required archetype.
/// [`QueryBorrow::iter`] will borrow the components from all archetypes and release them once the prepared query drops.
/// Subsequent calls to iter will use the same borrow.
impl<'w, Q, F> QueryBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
{
    /// Creates a new prepared query from a query, but does not allocate or lock anything.
    pub(super) fn new(
        world: &'w World,
        archetypes: &'w [ArchetypeId],
        fetch: &'w Q,
        filter: &'w F,
        old_tick: u32,
        new_tick: u32,
    ) -> Self {
        Self {
            prepared: SmallVec::new(),
            fetch,
            filter: And::new(
                RefFilter(filter),
                GatedFilter::new(Q::HAS_FILTER, fetch.filter()),
            ),
            old_tick,
            new_tick,
            world,
            archetypes,
        }
    }

    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> QueryIter<'q, 'w, Q, F>
    where
        'w: 'q,
        F: Filter<'q>,
    {
        QueryIter::new(self.iter_batched())
    }

    /// Returns the first item
    pub fn first<'q>(&'q mut self) -> Option<<Q as FetchItem<'q>>::Item>
    where
        'w: 'q,
        F: Filter<'q>,
    {
        self.iter().next()
    }

    /// Iterate all items matched by query and filter.
    pub fn iter_batched<'q>(&'q mut self) -> BatchedIter<'q, 'w, Q, F>
    where
        'w: 'q,
        F: Filter<'q>,
    {
        // Prepare all archetypes only if it is not already done
        // Clear previous borrows
        if self.prepared.len() != self.archetypes.len() {
            self.clear_borrows();
            self.prepared = self
                .archetypes
                .iter()
                .filter_map(|&arch_id| {
                    let arch = self.world.archetypes.get(arch_id);
                    if arch.is_empty() {
                        return None;
                    }
                    let data = FetchPrepareData {
                        world: self.world,
                        arch,
                        arch_id,
                    };

                    Some(PreparedArchetype {
                        arch_id,
                        arch,
                        fetch: self.fetch.prepare(data).unwrap(),
                    })
                })
                .collect();
        }

        BatchedIter::new(
            self.old_tick,
            self.new_tick,
            &self.filter,
            self.prepared.iter_mut(),
        )
    }

    /// Execute a closure for each item in the iterator.
    ///
    /// This is more efficient than `.iter().for_each(|v| {})` as the archetypes can be temporarily
    /// borrowed.
    pub fn for_each(&mut self, func: impl Fn(<Q as FetchItem<'_>>::Item) + Send + Sync)
    where
        F: for<'x> Filter<'x>,
    {
        self.clear_borrows();
        for &arch_id in self.archetypes {
            let arch = self.world.archetypes.get(arch_id);
            if arch.is_empty() {
                continue;
            }

            let data = FetchPrepareData {
                world: self.world,
                arch,
                arch_id,
            };

            let filter = FilterIter::new(arch.slots(), self.filter.prepare(arch, self.old_tick));

            let mut fetch = self.fetch.prepare(data).unwrap();

            let chunk: Chunks<Q, F> = Chunks {
                arch,
                fetch: &mut fetch,
                filter,
                new_tick: self.new_tick,
            };

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
    pub fn par_for_each<'q>(&'q mut self, func: impl Fn(<Q as FetchItem<'q>>::Item) + Send + Sync)
    where
        'w: 'q,
        Q::Prepared: Send,
        BatchedIter<'q, 'w, Q, F>: Send,
        F: Filter<'q>,
    {
        self.iter_batched()
            .par_bridge()
            .for_each(|v| v.for_each(&func))
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
        F: Filter<'q>,
    {
        self.iter_batched().map(|v| v.slots().len()).sum()
    }

    fn prepare_archetype(&mut self, arch_id: ArchetypeId) -> Option<usize> {
        let prepared = &mut self.prepared;

        if let Some(idx) = prepared.iter().position(|v| v.arch_id == arch_id) {
            Some(idx)
        } else {
            let arch = self.world.archetypes.get(arch_id);
            let data = FetchPrepareData {
                world: self.world,
                arch,
                arch_id,
            };

            let arch_id = *self.archetypes.iter().find(|&&v| v == arch_id)?;

            let fetch = self.fetch.prepare(data)?;

            prepared.push(PreparedArchetype {
                arch_id,
                arch,
                fetch,
            });

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
        F: Filter<'q>,
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
            let EntityLocation { arch_id, slot } = self.world.location(id)?;
            let idx = self.prepare_archetype(arch_id).ok_or_else(|| {
                Error::MissingComponent(
                    id,
                    find_missing_components(self.fetch, arch_id, self.world)
                        .next()
                        .unwrap_or_else(|| unknown_component().info()),
                )
                // let arch = self.world.archetypes.get(arch_id);
                // let mut buf = String::new();
                // self.fetch.describe(&mut buf).unwrap();

                // let mut components = Vec::new();
                // self.fetch.components(&mut components);
                // Error::UnmatchedFetch(
                //     id,
                //     buf,
                //     DifferenceIter::new(
                //         arch.components().map(|v| v.id()),
                //         components.into_iter(),
                //     )
                //     .map(|v| {
                //         self.world
                //             .get(v.id, is_component())
                //             .unwrap()
                //             .name()
                //             .to_string()
                //     })
                //     .collect_vec(),
                // )
            })?;

            idxs[i] = (idx, slot, id);
        }

        // Fetch all
        // All items will be initialized
        let mut items: [MaybeUninit<_>; C] = unsafe { MaybeUninit::uninit().assume_init() };
        for i in 0..C {
            let (idx, slot, id) = idxs[i];

            let prepared = &mut self.prepared[idx];

            // It is now guaranteed that the filter also matches since the archetype would not be
            // included otherwise.
            if !self
                .filter
                .prepare(prepared.arch, self.old_tick)
                .matches_slot(slot)
            {
                return Err(Error::MismatchedFilter(id));
            }

            // All entities are disjoint at this point
            let fetch = unsafe { &mut *(&mut prepared.fetch as *mut Q::Prepared) };
            items[i].write(unsafe { fetch.fetch(slot) });
        }

        unsafe {
            let items = mem::transmute_copy::<_, [<Q::Prepared as PreparedFetch>::Item; C]>(&items);
            Ok(items)
        }
    }

    /// Get the fetch items for an entity.
    pub fn get<'q>(&'q mut self, id: Entity) -> Result<<Q::Prepared as PreparedFetch>::Item>
    where
        F: Filter<'q>,
    {
        let EntityLocation { arch_id, slot } = self.world.location(id)?;

        let idx = self.prepare_archetype(arch_id).ok_or_else(|| {
            Error::MissingComponent(
                id,
                find_missing_components(self.fetch, arch_id, self.world)
                    .next()
                    .unwrap_or_else(|| unknown_component().info()),
            )
        })?;

        // Since `self` is a mutable references the borrow checker
        // guarantees this borrow is unique
        let p = &mut self.prepared[idx];

        // It is now guaranteed that the filter also matches since the archetype would not be
        // included otherwise.
        if !self
            .filter
            .prepare(p.arch, self.old_tick)
            .matches_slot(slot)
        {
            return Err(Error::MismatchedFilter(id));
        }

        let item = unsafe { p.fetch.fetch(slot) };

        Ok(item)
    }
}
