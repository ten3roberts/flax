use std::mem::MaybeUninit;

use smallvec::SmallVec;

use crate::{
    error::Result, All, Archetype, ArchetypeId, Entity, EntityLocation, Error, Fetch, Filter,
    PreparedFetch, World,
};

use super::iter::QueryIter;

pub struct PreparedArchetype<'w, Q> {
    pub(crate) id: ArchetypeId,
    pub(crate) arch: &'w Archetype,
    pub(crate) fetch: Q,
}

/// A lazily prepared query which borrows and hands out chunk iterators for
/// each archetype matched.
pub struct PreparedQuery<'w, Q, F = All>
where
    Q: Fetch<'w>,
{
    pub(crate) prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared>; 8]>,
    pub(crate) world: &'w World,
    pub(crate) archetypes: &'w [ArchetypeId],
    pub(crate) fetch: &'w Q,
    pub(crate) filter: &'w F,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
}

impl<'w, 'q, Q, F> IntoIterator for &'q mut PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q, 'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    type IntoIter = QueryIter<'w, 'q, Q, F>;

    fn into_iter(self) -> Self::IntoIter {
        // Prepared all archetypes
        self.prepared = self
            .archetypes
            .iter()
            .map(|&v| {
                let arch = self.world.archetype(v);
                PreparedArchetype {
                    id: v,
                    arch,
                    fetch: self
                        .fetch
                        .prepare(self.world, arch)
                        .expect("Mismathed archetype"),
                }
            })
            .collect();

        QueryIter {
            old_tick: self.old_tick,
            new_tick: self.new_tick,
            filter: self.filter,
            archetypes: self.prepared.iter_mut(),
            current: None,
        }
    }
}

impl<'w, Q, F> PreparedQuery<'w, Q, F>
where
    Q: Fetch<'w>,
{
    pub fn new(
        world: &'w World,
        archetypes: &'w [ArchetypeId],
        fetch: &'w Q,
        filter: &'w F,
        old_tick: u32,
        new_tick: u32,
    ) -> Self {
        Self {
            prepared: SmallVec::new(),
            filter,
            old_tick,
            new_tick,
            world,
            archetypes,
            fetch,
        }
    }

    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> QueryIter<'w, '_, Q, F>
    where
        'w: 'q,
        F: Filter<'q, 'w>,
    {
        self.into_iter()
    }

    fn prepare(&mut self, arch: ArchetypeId) -> Option<usize> {
        let world = self.world;
        let prepared = &mut self.prepared;

        if let Some(idx) = prepared.iter().position(|v| v.id == arch) {
            Some(idx)
        } else {
            let archetype = world.archetype(arch);
            let fetch = self.fetch.prepare(world, archetype)?;

            prepared.push(PreparedArchetype {
                id: arch,
                arch: archetype,
                fetch,
            });

            Some(prepared.len() - 1)
        }
    }

    /// Access any number of entites which are disjoint.
    /// Return None if any `id` is not disjoint.
    pub fn get_disjoint<'q, const C: usize>(
        &'q mut self,
        ids: [Entity; C],
    ) -> Result<[<Q::Prepared as PreparedFetch>::Item; C]>
    where
        Q: Fetch<'w>,
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
            let &EntityLocation { arch, slot } = self.world.location(id)?;
            idxs[i] = (
                self.prepare(arch).ok_or_else(|| {
                    let arch = self.world.archetype(arch);
                    Error::UnmatchedFetch(id, self.fetch.describe(), self.fetch.difference(arch))
                })?,
                slot,
            );
        }

        // Fetch all
        // All items will be initialized
        let mut items: [_; C] = unsafe { MaybeUninit::uninit().assume_init() };
        for i in 0..C {
            let (idx, slot) = idxs[i];
            items[i] = unsafe { self.prepared[idx].fetch.fetch(slot) };
        }

        Ok(items)
    }

    /// Get the fetch items for an entity.
    /// **Note**: Filters are ignored.
    pub fn get(&mut self, id: Entity) -> Result<<Q::Prepared as PreparedFetch>::Item>
    where
        Q: Fetch<'w>,
    {
        let &EntityLocation { arch, slot } = self.world.location(id)?;

        let idx = self.prepare(arch).ok_or_else(|| {
            let arch = self.world.archetype(arch);
            Error::UnmatchedFetch(id, self.fetch.describe(), self.fetch.difference(arch))
        })?;
        // Since `self` is a mutable references the borrow checker
        // guarantees this borrow is unique
        let p = &self.prepared[idx];
        let item = unsafe { p.fetch.fetch(slot) };

        Ok(item)
    }
}
