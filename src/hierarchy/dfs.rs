use core::mem;

use alloc::collections::BTreeMap;
use itertools::Itertools;
use smallvec::{smallvec, SmallVec};

use crate::{
    archetype::Slice,
    fetch::{FetchPrepareData, PreparedFetch},
    filter::{RefFilter, SliceFilter},
    And, ArchetypeChunks, Batch, Entity, Fetch, FetchItem, Filter, ParForEach, PreparedArchetype,
    World,
};

use super::{borrow::QueryBorrowState, DfsState};

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

    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> DfsIter<'w, 'q, Q, F>
    where
        F: Filter<'q>,
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.world.location(self.dfs_state.root).unwrap();
        let arch_index = *self.dfs_state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let arch = unsafe { &mut *(arch as *mut PreparedArchetype<Q::Prepared>) };

        let root_filter = And::new(
            self.state.filter.prepare(arch.arch, self.state.old_tick),
            SliceFilter(Slice::single(loc.slot)),
        );

        let stack = arch
            .chunks(self.state.old_tick, self.state.new_tick, root_filter)
            .collect();

        DfsIter {
            state: &self.state,
            archetypes: &mut self.prepared[..],
            stack,
            adj: &self.dfs_state.adj,
        }
    }

    pub fn cascade<'q, T, Fn: FnMut(<Q as FetchItem<'q>>::Item, &T) -> T>(
        &'q mut self,
        value: &T,
        mut func: Fn,
    ) where
        F: Filter<'q>,
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.world.location(self.dfs_state.root).unwrap();
        let arch_index = *self.dfs_state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let arch = unsafe { &mut *(arch as *mut PreparedArchetype<Q::Prepared>) };

        let root_filter = And::new(
            self.state.filter.prepare(arch.arch, self.state.old_tick),
            SliceFilter(Slice::single(loc.slot)),
        );

        let root = arch
            .chunks(self.state.old_tick, self.state.new_tick, root_filter)
            .next();

        if let Some(root) = root {
            Self::cascade_inner(
                &mut self.prepared,
                &self.dfs_state.adj,
                &self.state,
                root,
                value,
                &mut func,
            );
        }
    }

    fn cascade_inner<'q, T, Fn>(
        archetypes: &mut [PreparedArchetype<'w, Q::Prepared>],
        adj: &BTreeMap<Entity, Vec<usize>>,
        state: &'q QueryBorrowState<'w, Q, F>,
        mut batch: Batch<'q, Q::Prepared>,
        value: &T,
        func: &mut Fn,
    ) where
        Q: 'w,
        F: Filter<'q>,
        Fn: FnMut(<Q as FetchItem<'q>>::Item, &T) -> T,
        'w: 'q,
    {
        while let Some((id, item)) = batch.next_with_id() {
            let value = (func)(item, value);
            // Iterate the archetypes which contain all references to `id`
            for &arch_index in adj.get(&id).into_iter().flatten() {
                let arch = &mut archetypes[arch_index];

                // Promote the borrow of the fetch to 'q
                // This is safe because each borrow is disjoint
                let arch = unsafe { &mut *(arch as *mut PreparedArchetype<Q::Prepared>) };

                for batch in arch.chunks(
                    state.old_tick,
                    state.new_tick,
                    state.filter.prepare(arch.arch, state.old_tick),
                ) {
                    Self::cascade_inner(archetypes, adj, state, batch, &value, func)
                }
            }
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

    archetypes: &'q mut [PreparedArchetype<'w, Q::Prepared>],
    stack: SmallVec<[Batch<'q, Q::Prepared>; 8]>,
}

impl<'w, 'q, Q, F> DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q>,
    'w: 'q,
{
}

impl<'w, 'q, Q, F> Iterator for DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Filter<'q>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let chunk = self.stack.last_mut()?;
            if let Some((id, item)) = chunk.next_with_id() {
                // Add the children
                for &arch_index in self.adj.get(&id).into_iter().flatten() {
                    let arch = &mut self.archetypes[arch_index];

                    // Promote the borrow of the fetch to 'q
                    // This is safe because each borrow is disjoint
                    let arch = unsafe { &mut *(arch as *mut PreparedArchetype<Q::Prepared>) };

                    let chunks = arch.chunks(
                        self.state.old_tick,
                        self.state.new_tick,
                        self.state.filter.prepare(arch.arch, self.state.old_tick),
                    );

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
