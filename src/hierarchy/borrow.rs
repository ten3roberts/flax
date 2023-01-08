use core::{iter::Flatten, mem};

use alloc::collections::BTreeMap;
use smallvec::SmallVec;

use crate::{
    access_info,
    archetype::{Slice, Slot},
    fetch::{FetchPrepareData, PreparedEntities, PreparedFetch},
    filter::{FilterIter, RefFilter, TupleOr},
    All, ArchetypeChunks, ArchetypeId, Batch, Entity, EntityIds, Fetch, Filter, FilterWithFetch,
    Nothing, PreparedArchetype, World,
};

use super::DfsState;

pub(crate) struct QueryBorrowState<'w, Q, F>
where
    Q: Fetch<'w>,
{
    pub(crate) filter: FilterWithFetch<RefFilter<'w, F>, Q::Filter>,
    pub(crate) old_tick: u32,
    pub(crate) new_tick: u32,
}

pub struct DfsBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
{
    world: &'w World,
    fetch: &'w Q,

    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared>; 8]>,
    state: QueryBorrowState<'w, Q, F>,
    dfs_state: &'w DfsState,
}

impl<'w, Q, F> DfsBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
{
    pub(super) fn new(
        world: &'w World,
        fetch: &'w Q,
        state: QueryBorrowState<'w, Q, F>,
        dfs_state: &'w DfsState,
    ) -> Self {
        let prepared = dfs_state
            .archetypes
            .iter()
            .map(|&arch_id| {
                let arch = world.archetypes.get(arch_id);

                let data = FetchPrepareData {
                    world,
                    arch,
                    arch_id,
                };

                PreparedArchetype {
                    arch_id,
                    arch,
                    fetch: fetch.prepare(data).unwrap(),
                }
            })
            .collect();

        Self {
            world,
            fetch,
            prepared,
            state,
            dfs_state,
        }
    }

    pub fn iter<'q>(&'q mut self) -> DfsIter<'w, 'q, Q, F>
    where
        F: Filter<'q>,
        'w: 'q,
    {
        let loc = self.world.location(self.dfs_state.root).unwrap();
        let arch_index = *self.dfs_state.archetypes_index.get(&loc.arch_id).unwrap();
        let PreparedArchetype { arch, fetch, .. } = &mut self.prepared[arch_index];

        let chunk = Slice::single(loc.slot);

        // Fetch will never change and all calls are disjoint
        let fetch = unsafe { &mut *(fetch as *mut Q::Prepared) };

        // Set the chunk as visited
        unsafe { fetch.set_visited(chunk, self.state.new_tick) }
        let chunk = Batch::new(arch, fetch, chunk);

        DfsIter {
            state: &self.state,
            archetypes: &mut self.prepared[..],
            stack: vec![chunk],
            adj: &self.dfs_state.adj,
        }
    }
}

struct BatchesWithId<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q>,
    'w: 'q,
{
    chunks: ArchetypeChunks<'w, 'q, Q, F>,
    current: Option<Batch<'q, Q::Prepared>>,
}

impl<'w, 'q, Q, F> Iterator for BatchesWithId<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q>,
{
    type Item = (Entity, <Q::Prepared as PreparedFetch<'q>>::Item);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current) = self.current.as_mut() {
                if let item @ Some(_) = current.next_with_id() {
                    return item;
                }
            }

            self.current = Some(self.chunks.next()?);
        }
    }
}

pub struct DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q>,
    'w: 'q,
{
    adj: &'q BTreeMap<Entity, Vec<usize>>,
    state: &'q QueryBorrowState<'w, Q, F>,

    pub(crate) archetypes: &'q mut [PreparedArchetype<'w, Q::Prepared>],
    stack: Vec<Batch<'q, Q::Prepared>>,
}

impl<'w, 'q, Q, F> Iterator for DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        // Handle root manually
        // if let Some((root, root_id)) = self.root.take() {
        //     let PreparedArchetype { arch, fetch, .. } = &mut self.archetypes[0];

        //     // Promote the borrow of the fetch to 'q
        //     // This is safe because each borrow is disjoint
        //     let fetch: &'q mut Q::Prepared = unsafe { mem::transmute(fetch) };
        //     unsafe { fetch.set_visited(Slice::single(root), self.state.new_tick) }

        //     // Add the children
        //     for &arch_index in self.adj.get(&root_id).into_iter().flatten() {
        //         let PreparedArchetype { arch, fetch, .. } = &mut self.archetypes[arch_index];

        //         // Promote the borrow of the fetch to 'q
        //         // This is safe because each borrow is disjoint
        //         let fetch = unsafe { mem::transmute(fetch) };

        //         let filter = FilterIter::new(
        //             arch.slots(),
        //             self.state.filter.prepare(arch, self.state.old_tick),
        //         );

        //         let chunks: ArchetypeChunks<Q, F> = ArchetypeChunks {
        //             arch,
        //             fetch,
        //             filter,
        //             new_tick: self.state.new_tick,
        //         };

        //         self.stack.push(BatchesWithId {
        //             chunks,
        //             current: None,
        //         });
        //     }

        //     return Some(unsafe { fetch.fetch(root) });
        // }

        loop {
            let chunk = self.stack.last_mut()?;
            if let Some((id, item)) = chunk.next_with_id() {
                // Add the children
                for &arch_index in self.adj.get(&id).into_iter().flatten() {
                    let PreparedArchetype { arch, fetch, .. } = &mut self.archetypes[arch_index];

                    // Promote the borrow of the fetch to 'q
                    // This is safe because each borrow is disjoint
                    let fetch = unsafe { mem::transmute(fetch) };

                    let filter = FilterIter::new(
                        arch.slots(),
                        self.state.filter.prepare(arch, self.state.old_tick),
                    );

                    let chunks: ArchetypeChunks<Q, F> = ArchetypeChunks {
                        arch,
                        fetch,
                        filter,
                        new_tick: self.state.new_tick,
                    };

                    self.stack.extend(chunks);
                }

                return Some(item);
            } else {
                // The top of the stack is exhausted
                self.stack.pop();
            }
        }
    }
}
