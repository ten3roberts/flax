use crate::{
    fetch::FetchAccessData, filter::Filtered, Access, AccessKind, ArchetypeId, ComponentValue,
};
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

use super::{borrow::QueryBorrowState, Batch, PreparedArchetype, QueryStrategy};

/// Iterate a hierarchy in depth-first order
pub struct Dfs {
    root: Entity,
    relation: Entity,

    archetype_gen: u32,
    state: DfsState,
}

#[derive(Default, Debug, Clone)]
struct DfsState {
    archetypes: Vec<ArchetypeId>,
    archetypes_index: BTreeMap<ArchetypeId, usize>,
    adj: BTreeMap<Entity, Vec<usize>>,
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

    fn clear(&mut self) {
        self.archetypes.clear();
        self.archetypes_index.clear();
        self.adj.clear();
    }
}

impl Dfs {
    /// Iterate a hierarchy in depth-first order from `root`
    pub fn new<T: ComponentValue>(relation: impl RelationExt<T>, root: Entity) -> Self {
        Self {
            relation: relation.id(),
            root,

            archetype_gen: 0,
            state: DfsState::default(),
        }
    }

    fn update_state<'w, Q: Fetch<'w>, F: Fetch<'w>>(
        relation: Entity,
        root: Entity,
        result: &mut DfsState,
        world: &crate::World,
        fetch: &'w Filtered<Q, F>,
    ) {
        result.clear();
        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);

        let archetypes = &world.archetypes;

        struct SearchState<'a> {
            archetypes: &'a Archetypes,
            searcher: &'a ArchetypeSearcher,
            relation: Entity,
            visited: BTreeSet<ArchetypeId>,
        }

        fn inner<'w, F: Fetch<'w>>(
            state: &mut SearchState,
            result: &mut DfsState,
            fetch: &'w F,
            id: Entity,
        ) {
            // Find all archetypes for the children of parent
            let key = ComponentKey::new(state.relation, Some(id));

            let mut searcher = state.searcher.clone();
            searcher.add_required(key);

            let mut children = Vec::new();
            searcher.find_archetypes(state.archetypes, |arch_id, arch| {
                if !fetch.filter_arch(arch) {
                    return;
                }

                let arch_index = match result.archetypes_index.entry(arch_id) {
                    btree_map::Entry::Vacant(slot) => {
                        let idx = result.archetypes.len();
                        result.archetypes.push(arch_id);
                        *slot.insert(idx)
                    }
                    btree_map::Entry::Occupied(_) => panic!("Cycle"),
                };

                for (slot, &id) in arch.entities().iter().enumerate() {
                    let loc = EntityLocation { slot, arch_id };
                    inner(state, result, fetch, id);
                }

                children.push(arch_index);
            });

            assert!(result.adj.insert(id, children).is_none());
        }

        let loc = world.location(root).unwrap();

        let mut state = SearchState {
            archetypes,
            searcher: &searcher,
            relation,
            visited: Default::default(),
        };

        archetypes.get(loc.arch_id);
        result.insert_arch(loc.arch_id);

        inner(&mut state, result, fetch, root);
    }
}

impl<'w, Q, F> QueryStrategy<'w, Q, F> for Dfs
where
    Q: 'w + Fetch<'w>,
    F: 'w + Fetch<'w>,
{
    type Borrow = DfsBorrow<'w, Q, F>;

    fn borrow(&'w mut self, query_state: QueryBorrowState<'w, Q, F>, dirty: bool) -> Self::Borrow {
        if dirty {
            Self::update_state(
                self.relation,
                self.root,
                &mut self.state,
                query_state.world,
                query_state.fetch,
            );
        }

        DfsBorrow::new(query_state, self)
    }

    fn access(&self, world: &'w World, fetch: &'w Filtered<Q, F>) -> Vec<crate::Access> {
        let mut state = DfsState::default();
        Self::update_state(self.relation, self.root, &mut state, world, fetch);

        state
            .archetypes
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

/// Borrowed state for [`Dfs`] strategy
pub struct DfsBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 8]>,
    state: QueryBorrowState<'w, Q, F>,
    dfs: &'w Dfs,
}

impl<'w, Q, F> DfsBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    fn new(query_state: QueryBorrowState<'w, Q, F>, dfs: &'w Dfs) -> Self {
        let prepared = dfs
            .state
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
            dfs,
        }
    }

    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> DfsIter<'w, 'q, Q, F>
    where
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.state.world.location(self.dfs.root).unwrap();
        let arch_index = *self.dfs.state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let p = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };
        let chunk = match p.manual_chunk(Slice::single(loc.slot)) {
            Some(v) => smallvec![v],
            None => smallvec![],
        };

        DfsIter {
            archetypes: &mut self.prepared[..],
            stack: chunk,
            adj: &self.dfs.state.adj,
        }
    }

    /// Traverse the hierarchy recursively, visiting each entity with the return value for the parent
    pub fn traverse<'q, T, Fn: FnMut(<Q as FetchItem<'q>>::Item, &T) -> T>(
        &'q mut self,
        value: &T,
        mut func: Fn,
    ) where
        'w: 'q,
    {
        // Safety: the iterator will not borrow this archetype again
        let loc = self.state.world.location(self.dfs.root).unwrap();
        let arch_index = *self.dfs.state.archetypes_index.get(&loc.arch_id).unwrap();

        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let arch = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };

        let root = arch.manual_chunk(Slice::single(loc.slot));

        if let Some(root) = root {
            Self::traverse_inner(
                &mut self.prepared,
                &self.dfs.state.adj,
                root,
                value,
                &mut func,
            );
        }
    }

    fn traverse_inner<'q, T, Fn>(
        archetypes: &mut [PreparedArchetype<'w, Q::Prepared, F::Prepared>],
        adj: &BTreeMap<Entity, Vec<usize>>,
        mut batch: Batch<'q, Q::Prepared, F::Prepared>,
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
                let p = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };

                for batch in p.chunks() {
                    Self::traverse_inner(archetypes, adj, batch, &value, func)
                }
            }
        }
    }
}

/// Iterate a hierarchy in depth-first order
pub struct DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    adj: &'q BTreeMap<Entity, Vec<usize>>,

    archetypes: &'q mut [PreparedArchetype<'w, Q::Prepared, F::Prepared>],
    stack: SmallVec<[Batch<'q, Q::Prepared, F::Prepared>; 8]>,
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
                    let p = unsafe { &mut *(p as *mut PreparedArchetype<_, _>) };

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
