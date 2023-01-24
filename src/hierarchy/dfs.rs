use crate::{ArchetypeId, ComponentValue};
use alloc::{
    collections::{btree_map, BTreeMap, BTreeSet},
    vec::Vec,
};
use smallvec::{smallvec, SmallVec};

use crate::{
    archetype::{Archetype, Slice},
    entity::EntityLocation,
    fetch::PreparedFetch,
    query::ArchetypeSearcher,
    Archetypes, ComponentKey, Entity, Fetch, FetchItem, RelationExt, World,
};

type AdjMap<'a> = BTreeMap<Entity, SmallVec<[(ArchetypeId, &'a Archetype); 8]>>;

use super::{borrow::QueryBorrowState, Batch, PreparedArchetype, QueryState, QueryStrategy};

/// Iterate a hierarchy in depth-first order
pub struct Dfs {
    root: Entity,
    relation: Entity,
}

impl Dfs {
    /// Iterate a hierarchy in depth-first order from `root`
    pub fn new<T: ComponentValue>(relation: impl RelationExt<T>, root: Entity) -> Self {
        Self {
            relation: relation.id(),
            root,
        }
    }
}

impl<Q> QueryStrategy<Q> for Dfs
where
    Q: 'static + for<'x> Fetch<'x>,
{
    type State = DfsState;

    fn state(&self, world: &World, fetch: &Q) -> Self::State {
        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);

        let archetypes = &world.archetypes;

        struct SearchState<'a, F> {
            archetypes: &'a Archetypes,
            searcher: &'a ArchetypeSearcher,
            fetch: &'a F,
            relation: Entity,
            result: DfsState,
            visited: BTreeSet<ArchetypeId>,
        }

        fn inner<F: for<'x> Fetch<'x>>(
            state: &mut SearchState<F>,
            loc: EntityLocation,
            _: &Archetype,
            _: usize,
            id: Entity,
        ) {
            // Find all archetypes for the children of parent
            let key = ComponentKey::new(state.relation, Some(id));

            let mut searcher = state.searcher.clone();
            searcher.add_required(key);

            let mut children = Vec::new();
            searcher.find_archetypes(state.archetypes, |arch_id, arch| {
                if !state.fetch.filter_arch(arch) {
                    return;
                }

                let arch_index = match state.result.archetypes_index.entry(arch_id) {
                    btree_map::Entry::Vacant(slot) => {
                        let idx = state.result.archetypes.len();
                        state.result.archetypes.push(arch_id);
                        *slot.insert(idx)
                    }
                    btree_map::Entry::Occupied(_) => panic!("Cycle"),
                };

                for (slot, &id) in arch.entities().iter().enumerate() {
                    let loc = EntityLocation { slot, arch_id };
                    inner(state, loc, arch, arch_index, id);
                }

                children.push(arch_index);
            });

            assert!(state.result.adj.insert(id, children).is_none());
        }

        let loc = world.location(self.root).unwrap();

        let mut state = SearchState {
            archetypes,
            searcher: &searcher,
            fetch,
            relation: self.relation,
            result: DfsState {
                archetypes: Default::default(),
                adj: Default::default(),
                archetypes_index: Default::default(),
                root: self.root,
            },
            visited: Default::default(),
        };

        let arch = archetypes.get(loc.arch_id);
        let arch_index = state.result.insert_arch(loc.arch_id);

        inner(&mut state, loc, arch, arch_index, self.root);

        state.result
    }
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct DfsState {
    archetypes: Vec<ArchetypeId>,
    archetypes_index: BTreeMap<ArchetypeId, usize>,
    adj: BTreeMap<Entity, Vec<usize>>,
    root: Entity,
}

impl DfsState {
    fn insert_arch(&mut self, arch_id: ArchetypeId) -> usize {
        match self.archetypes_index.entry(arch_id) {
            btree_map::Entry::Vacant(slot) => {
                let idx = self.archetypes.len();
                self.archetypes.push(arch_id);
                *slot.insert(idx)
            }
            btree_map::Entry::Occupied(mut slot) => *slot.get_mut(),
        }
    }
}

impl<'w, Q> QueryState<'w, Q> for DfsState
where
    Q: Fetch<'w> + 'w,
{
    type Borrow = DfsBorrow<'w, Q>;

    fn borrow(&'w self, query_state: QueryBorrowState<'w, Q>) -> Self::Borrow {
        DfsBorrow::new(query_state, self)
    }
}

/// Borrowed state for [`Dfs`] strategy
pub struct DfsBorrow<'w, Q>
where
    Q: Fetch<'w>,
{
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared>; 8]>,
    state: QueryBorrowState<'w, Q>,
    dfs_state: &'w DfsState,
}

impl<'w, Q> DfsBorrow<'w, Q>
where
    Q: Fetch<'w>,
{
    fn new(query_state: QueryBorrowState<'w, Q>, dfs_state: &'w DfsState) -> Self {
        let prepared = dfs_state
            .archetypes
            .iter()
            .map(|&arch_id| {
                let arch = query_state.world.archetypes.get(arch_id);
                query_state.prepare_fetch(arch, arch_id).unwrap()
            })
            .collect();

        Self {
            prepared,
            state: query_state,
            dfs_state,
        }
    }

    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> DfsIter<'w, 'q, Q>
    where
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.state.world.location(self.dfs_state.root).unwrap();
        let arch_index = *self.dfs_state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let p = unsafe { &mut *(arch as *mut PreparedArchetype<_>) };
        let chunk = match p.manual_chunk(Slice::single(loc.slot)) {
            Some(v) => smallvec![v],
            None => smallvec![],
        };

        DfsIter {
            archetypes: &mut self.prepared[..],
            stack: chunk,
            adj: &self.dfs_state.adj,
        }
    }

    /// Cascade recursively, visiting each entity with the return value for the parent
    pub fn cascade<'q, T, Fn: FnMut(<Q as FetchItem<'q>>::Item, &T) -> T>(
        &'q mut self,
        value: &T,
        mut func: Fn,
    ) where
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.state.world.location(self.dfs_state.root).unwrap();
        let arch_index = *self.dfs_state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let arch = unsafe { &mut *(arch as *mut PreparedArchetype<_>) };

        let root = arch.manual_chunk(Slice::single(loc.slot));

        if let Some(root) = root {
            Self::cascade_inner(
                &mut self.prepared,
                &self.dfs_state.adj,
                root,
                value,
                &mut func,
            );
        }
    }

    fn cascade_inner<'q, T, Fn>(
        archetypes: &mut [PreparedArchetype<'w, Q::Prepared>],
        adj: &BTreeMap<Entity, Vec<usize>>,
        mut batch: Batch<'q, Q::Prepared>,
        value: &T,
        func: &mut Fn,
    ) where
        Q: 'w,
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

                for batch in p.chunks() {
                    Self::cascade_inner(archetypes, adj, batch, &value, func)
                }
            }
        }
    }
}

/// Iterate a hierarchy in depth-first order
pub struct DfsIter<'w, 'q, Q>
where
    Q: Fetch<'w>,
    'w: 'q,
{
    adj: &'q BTreeMap<Entity, Vec<usize>>,

    archetypes: &'q mut [PreparedArchetype<'w, Q::Prepared>],
    stack: SmallVec<[Batch<'q, Q::Prepared>; 8]>,
}

impl<'w, 'q, Q> Iterator for DfsIter<'w, 'q, Q>
where
    Q: Fetch<'w>,
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

                    let chunks = p.chunks();

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
