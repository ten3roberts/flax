use std::mem::{self, MaybeUninit};

use smallvec::SmallVec;

use crate::{
    entity::EntityLocation, error::Result, fetch::FetchPrepareData, fetch::PreparedFetch,
    filter::All, filter::And, filter::GatedFilter, Archetype, ArchetypeId, Entity, Error, Fetch,
    FetchItem, Filter, World,
};

use super::{
    iter::{BatchedIter, QueryIter},
    FilterWithFetch,
};

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
    pub(crate) filter: FilterWithFetch<&'w F, Q::Filter>,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
}

impl<'w, 'q, Q, F> IntoIterator for &'q mut QueryBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    &'w F: Filter<'q>,
    'w: 'q,
{
    type Item = <Q as FetchItem<'q>>::Item;

    type IntoIter = QueryIter<'q, 'w, Q, F>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Represents a query that is bounded to the lifetime of the world.
/// Contains the borrows and holds them until it is dropped.
///
/// The borrowing is lazy, as such, calling [`PreparedQuery::get`] will only borrow the one required archetype.
/// [`PreparedQuery::iter`] will borrow the components from all archetypes and release them once the prepared query drops.
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
            filter: And::new(filter, GatedFilter::new(Q::HAS_FILTER, fetch.filter())),
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
        &'w F: Filter<'q>,
    {
        QueryIter {
            inner: self.iter_batched().flatten(),
        }
    }

    /// Returns the first item
    pub fn first<'q>(&'q mut self) -> Option<<Q as FetchItem<'q>>::Item>
    where
        'w: 'q,
        &'w F: Filter<'q>,
    {
        self.iter().next()
    }

    /// Iterate all items matched by query and filter.
    pub fn iter_batched<'q>(&'q mut self) -> BatchedIter<'q, 'w, Q, F>
    where
        'w: 'q,
        &'w F: Filter<'q>,
    {
        // Prepare all archetypes only if it is not already done
        // Clear previous borrows
        if self.prepared.len() != self.archetypes.len() {
            self.prepared.clear();
            self.prepared = self
                .archetypes
                .iter()
                .filter_map(|&arch_id| {
                    let arch = {
                        let ref this = self.world;
                        this.archetypes.get(arch_id)
                    };
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
                        fetch: self.fetch.prepare(data).expect("Mismathed archetype"),
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

    /// Consumes the iterator and returns the number of entities visited.
    /// Faster than `self.iter().count()`
    pub fn count<'q>(&'q mut self) -> usize
    where
        'w: 'q,
        &'w F: Filter<'q>,
    {
        self.iter_batched().map(|v| v.slots().len()).sum()
    }

    fn prepare_archetype(&mut self, arch_id: ArchetypeId) -> Option<usize> {
        let world = self.world;
        let prepared = &mut self.prepared;

        if let Some(idx) = prepared.iter().position(|v| v.arch_id == arch_id) {
            Some(idx)
        } else {
            let arch = {
                let ref this = world;
                this.archetypes.get(arch_id)
            };
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
        &'w F: Filter<'q>,
    {
        let mut sorted = ids;
        sorted.sort();
        if sorted.windows(C).any(|v| v[0] == v[1]) {
            // Not disjoint
            return Err(Error::Disjoint(ids.to_vec()));
        }

        // Prepare all
        let mut idxs = [(0, 0); C];

        for i in 0..C {
            let id = ids[i];
            let EntityLocation {
                arch: arch_id,
                slot,
            } = self.world.location(id)?;
            idxs[i] = (
                self.prepare_archetype(arch_id).ok_or_else(|| {
                    let arch = {
                        let ref this = self.world;
                        this.archetypes.get(arch_id)
                    };
                    let mut buf = String::new();
                    self.fetch.describe(&mut buf).unwrap();

                    Error::UnmatchedFetch(
                        id,
                        buf,
                        self.fetch.difference(FetchPrepareData {
                            world: self.world,
                            arch,
                            arch_id,
                        }),
                    )
                })?,
                slot,
            );
        }

        // Fetch all
        // All items will be initialized
        let mut items: [MaybeUninit<_>; C] = unsafe { MaybeUninit::uninit().assume_init() };
        for i in 0..C {
            let (idx, slot) = idxs[i];

            // All entities are disjoint at this point
            let fetch = unsafe { &mut *(&mut self.prepared[idx].fetch as *mut Q::Prepared) };
            items[i].write(unsafe { fetch.fetch(slot) });
        }

        unsafe {
            let items = mem::transmute_copy::<_, [<Q::Prepared as PreparedFetch>::Item; C]>(&items);
            Ok(items)
        }
    }

    /// Get the fetch items for an entity.
    /// **Note**: Components from filters are included in the match, such as `With`
    pub fn get(&mut self, id: Entity) -> Result<<Q::Prepared as PreparedFetch>::Item> {
        let EntityLocation {
            arch: arch_id,
            slot,
        } = self.world.location(id)?;

        let idx = self.prepare_archetype(arch_id).ok_or_else(|| {
            let arch = {
                let ref this = self.world;
                this.archetypes.get(arch_id)
            };
            let mut buf = String::new();
            self.fetch.describe(&mut buf).unwrap();

            Error::UnmatchedFetch(
                id,
                buf,
                self.fetch.difference(FetchPrepareData {
                    world: self.world,
                    arch,
                    arch_id,
                }),
            )
        })?;

        // Since `self` is a mutable references the borrow checker
        // guarantees this borrow is unique
        let p = &mut self.prepared[idx];
        let item = unsafe { p.fetch.fetch(slot) };

        Ok(item)
    }
}
