use alloc::collections::BTreeMap;
use smallvec::{smallvec, SmallVec};

use crate::{
    archetype::Slice,
    fetch::{FetchPrepareData, PreparedFetch},
    filter::Filtered,
    Batch, Entity, Fetch, FetchItem, PreparedArchetype, World,
};

use super::{borrow::QueryBorrowState, DfsState};

pub struct DfsBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    world: &'w World,
    fetch: &'w Filtered<Q, F>,

    prepared: SmallVec<[PreparedArchetype<'w, Filtered<Q::Prepared, F::Prepared>>; 8]>,
    state: QueryBorrowState,
    dfs_state: &'w DfsState,
}

impl<'w, Q, F> DfsBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    pub(super) fn new(
        world: &'w World,
        fetch: &'w Filtered<Q, F>,
        state: QueryBorrowState,
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
                    old_tick: state.old_tick,
                    new_tick: state.new_tick,
                };

                let fetch = fetch.prepare(data).unwrap();

                PreparedArchetype {
                    arch_id,
                    arch,
                    fetch,
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
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.world.location(self.dfs_state.root).unwrap();
        let arch_index = *self.dfs_state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let p = unsafe { &mut *(arch as *mut PreparedArchetype<_>) };
        let chunk = match p.manual_chunk(
            Slice::single(loc.slot),
            self.state.old_tick,
            self.state.new_tick,
        ) {
            Some(v) => smallvec![v],
            None => smallvec![],
        };

        DfsIter {
            world: self.world,
            state: &self.state,
            archetypes: &mut self.prepared[..],
            stack: chunk,
            adj: &self.dfs_state.adj,
        }
    }

    pub fn cascade<'q, T, Fn: FnMut(<Q as FetchItem<'q>>::Item, &T) -> T>(
        &'q mut self,
        value: &T,
        mut func: Fn,
    ) where
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.world.location(self.dfs_state.root).unwrap();
        let arch_index = *self.dfs_state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let arch = unsafe { &mut *(arch as *mut PreparedArchetype<_>) };

        let root = arch.manual_chunk(
            Slice::single(loc.slot),
            self.state.old_tick,
            self.state.new_tick,
        );

        if let Some(root) = root {
            Self::cascade_inner(
                self.world,
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
        world: &'q World,
        archetypes: &mut [PreparedArchetype<'w, Filtered<Q::Prepared, F::Prepared>>],
        adj: &BTreeMap<Entity, Vec<usize>>,
        state: &'q QueryBorrowState,
        mut batch: Batch<'q, Filtered<Q::Prepared, F::Prepared>>,
        value: &T,
        func: &mut Fn,
    ) where
        Q: 'w,
        F: 'w,
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
                let p = unsafe { &mut *(arch as *mut PreparedArchetype<_>) };

                for batch in p.chunks(state.old_tick, state.new_tick) {
                    Self::cascade_inner(world, archetypes, adj, state, batch, &value, func)
                }
            }
        }
    }
}

pub struct DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    world: &'w World,
    adj: &'q BTreeMap<Entity, Vec<usize>>,
    state: &'q QueryBorrowState,

    archetypes: &'q mut [PreparedArchetype<'w, Filtered<Q::Prepared, F::Prepared>>],
    stack: SmallVec<[Batch<'q, Filtered<Q::Prepared, F::Prepared>>; 8]>,
}

impl<'w, 'q, Q, F> DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
}

impl<'w, 'q, Q, F> Iterator for DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let chunk = self.stack.last_mut()?;
            if let Some((id, item)) = chunk.next_with_id() {
                // Add the children
                for &arch_index in self.adj.get(&id).into_iter().flatten() {
                    let p = &mut self.archetypes[arch_index];

                    // Promote the borrow of the fetch to 'q
                    // This is safe because each borrow is disjoint
                    let p = unsafe { &mut *(p as *mut PreparedArchetype<_>) };

                    let chunks = p.chunks(self.state.old_tick, self.state.new_tick);

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
