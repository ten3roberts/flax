use core::{iter::Flatten, slice::IterMut};

use crate::{
    archetype::{Slice, Slot},
    fetch::PreparedFetch,
    filter::Filtered,
    hierarchy::{ArchetypeChunks, Batch, PreparedArchetype},
    Archetype, Entity, Fetch, World,
};

/// The query iterator
pub struct QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    iter: Flatten<BatchedIter<'q, 'w, Q, F>>,
}

impl<'q, 'w, Q, F> QueryIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    #[inline(always)]
    pub(crate) fn new(iter: BatchedIter<'q, 'w, Q, F>) -> Self {
        Self {
            iter: iter.flatten(),
            // current: None,
        }
    }
}

impl<'w, 'q, Q, F> Iterator for QueryIter<'q, 'w, Q, F>
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
pub struct BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    world: &'w World,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
    pub(crate) archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared, F::Prepared>>,
    pub(crate) current: Option<ArchetypeChunks<'q, Q::Prepared, F::Prepared>>,
}

impl<'q, 'w, Q, F> BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    pub(super) fn new(
        world: &'w World,
        old_tick: u32,
        new_tick: u32,
        archetypes: IterMut<'q, PreparedArchetype<'w, Q::Prepared, F::Prepared>>,
    ) -> Self {
        Self {
            world,
            old_tick,
            new_tick,
            archetypes,
            current: None,
        }
    }
}

impl<'w, 'q, Q, F> Iterator for BatchedIter<'q, 'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    type Item = Batch<'q, Q::Prepared, F::Prepared>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(chunk) = self.current.as_mut() {
                if let item @ Some(..) = chunk.next() {
                    return item;
                }
            }

            let PreparedArchetype { arch, fetch, .. } = self.archetypes.next()?;

            // let filter = FilterIter::new(
            //     arch.slots(),
            //     self.filter.prepare(
            //         FetchPrepareData {
            //             world: self.world,
            //             arch,
            //             arch_id: *arch_id,
            //             old_tick: self.old_tick,
            //             new_tick: todo!(),
            //         },
            //         self.old_tick,
            //     ),
            // );

            let chunk = ArchetypeChunks {
                slots: arch.slots(),
                arch,
                fetch,
            };

            self.current = Some(chunk);
        }
    }
}
