use core::marker::PhantomData;

use crate::{
    archetypes::Archetypes, fetch::FetchAccessData, filter::Filtered, Access, AccessKind,
    ArchetypeId, ComponentValue,
};
use alloc::{
    collections::{btree_map, BTreeMap},
    vec::Vec,
};
use smallvec::{smallvec, SmallVec};

use crate::{
    archetype::Slice, fetch::PreparedFetch, ArchetypeSearcher, ComponentKey, Entity, Fetch,
    FetchItem, RelationExt, World,
};

use super::{borrow::QueryBorrowState, Batch, PreparedArchetype, QueryStrategy};

/// Iterate a hierarchy in depth-first order
pub struct Dfs<T> {
    root: Entity,
    relation: Entity,

    state: DfsState,

    marker: PhantomData<T>,
}

#[derive(Default, Debug, Clone)]
struct DfsState {
    archetypes: Vec<ArchetypeId>,
    archetypes_index: BTreeMap<ArchetypeId, usize>,
    adj: AdjMap,
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

    fn update<'w, Q: Fetch<'w>, F: Fetch<'w>>(
        &mut self,
        relation: Entity,
        root: Entity,
        world: &crate::World,
        fetch: &'w Filtered<Q, F>,
    ) {
        self.clear();

        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);

        let archetypes = &world.archetypes;

        struct SearchState<'a> {
            archetypes: &'a Archetypes,
            searcher: &'a ArchetypeSearcher,
            relation: Entity,
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

            let mut children = SmallVec::new();
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
                    btree_map::Entry::Occupied(_) => {
                        return;
                    }
                };

                for &id in arch.entities().iter() {
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
        };

        archetypes.get(loc.arch_id);
        self.insert_arch(loc.arch_id);

        inner(&mut state, self, fetch, root);
    }

    fn clear(&mut self) {
        self.archetypes.clear();
        self.archetypes_index.clear();
        self.adj.clear();
    }
}

impl<T: ComponentValue> Dfs<T> {
    /// Iterate a hierarchy in depth-first order from `root`
    pub fn new(relation: impl RelationExt<T>, root: Entity) -> Self {
        Self {
            relation: relation.id(),
            root,

            state: DfsState::default(),
            marker: PhantomData,
        }
    }
}

impl<'w, Q, F, T: ComponentValue> QueryStrategy<'w, Q, F> for Dfs<T>
where
    Q: 'w + Fetch<'w>,
    F: 'w + Fetch<'w>,
{
    type Borrow = DfsBorrow<'w, T, Q, F>;

    fn borrow(&'w mut self, query_state: QueryBorrowState<'w, Q, F>, dirty: bool) -> Self::Borrow {
        if dirty {
            self.state.update(
                self.relation,
                self.root,
                query_state.world,
                query_state.fetch,
            )
        }

        DfsBorrow::new(query_state, self)
    }

    fn access(&self, world: &'w World, fetch: &'w Filtered<Q, F>) -> Vec<crate::Access> {
        let mut state = DfsState::default();
        state.update(self.relation, self.root, world, fetch);

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
pub struct DfsBorrow<'w, T, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 8]>,
    state: QueryBorrowState<'w, Q, F>,
    dfs: &'w Dfs<T>,
}

impl<'w, T, Q, F> DfsBorrow<'w, T, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    T: ComponentValue,
{
    fn new(query_state: QueryBorrowState<'w, Q, F>, dfs: &'w Dfs<T>) -> Self {
        let prepared = dfs
            .state
            .archetypes
            .iter()
            .map(|&arch_id| {
                let arch = query_state.world.archetypes.get(arch_id);
                query_state.prepare_fetch(arch_id, arch).unwrap()
            })
            .collect();

        Self {
            prepared,
            state: query_state,
            dfs,
        }
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use crate::{child_of, entity_ids, name, CommandBuffer, DfsRoots, FetchExt, Query};

    use super::*;
}
