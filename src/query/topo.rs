use core::iter::Flatten;

use alloc::{
    collections::{btree_map, BTreeMap, BTreeSet},
    vec::Vec,
};
use smallvec::SmallVec;

use crate::{
    fetch::{FetchAccessData, PreparedFetch},
    Access, AccessKind, ArchetypeId, BatchedIter, ComponentValue, Entity, Fetch, PreparedArchetype,
    QueryStrategy, RelationExt,
};

use super::{borrow::QueryBorrowState, ArchetypeSearcher};

/// Visit entities in topological order following `relation`.
///
/// Cycles are not visited.
///
/// Links where the fetch is not satisfied, e.g; missing components, will "fall-through" and
/// affect the ordering, but not be returned by the iteration.
pub struct Topo {
    state: State,
    relation: Entity,
}

#[derive(Default, Debug, Clone)]
struct State {
    archetypes: Vec<ArchetypeId>,
    order: Vec<usize>,
    archetypes_index: BTreeMap<ArchetypeId, usize>,
}

impl State {
    fn update<'w, Q: Fetch<'w>>(&mut self, relation: Entity, world: &crate::World, fetch: &'w Q) {
        self.clear();
        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);
        // Maps each entity to all archetypes of its children
        let mut deps: BTreeMap<_, _> = BTreeMap::new();

        searcher.find_archetypes(&world.archetypes, |arch_id, arch| {
            if !fetch.filter_arch(arch) {
                return;
            }

            match self.archetypes_index.entry(arch_id) {
                btree_map::Entry::Vacant(slot) => {
                    let idx = self.archetypes.len();
                    self.archetypes.push(arch_id);
                    slot.insert(idx);
                }
                btree_map::Entry::Occupied(_) => panic!("Duplicate archetype"),
            };

            // Find dependencies
            let arch_deps: Vec<_> = arch
                .relations_like(relation)
                .map(|(key, _)| {
                    assert_eq!(key.id, relation);
                    let object = key.object().unwrap();
                    let loc = world.location(object).unwrap();
                    loc.arch_id
                })
                .collect();

            if !arch_deps.is_empty() {
                deps.insert(arch_id, arch_deps);
            }
        });

        fn sort(
            order: &mut Vec<usize>,
            visited: &mut BTreeSet<ArchetypeId>,
            index: &BTreeMap<ArchetypeId, usize>,
            deps: &BTreeMap<ArchetypeId, Vec<ArchetypeId>>,
            arch_id: ArchetypeId,
        ) {
            if !visited.insert(arch_id) {
                return;
            }

            // Make sure all dependencies i.e; parents, are visited first
            for &dep in deps.get(&arch_id).into_iter().flatten() {
                sort(order, visited, index, deps, dep);
            }

            if let Some(&arch_index) = index.get(&arch_id) {
                order.push(arch_index);
            }
        }

        let mut visited = BTreeSet::new();
        for &arch_id in self.archetypes.iter() {
            sort(
                &mut self.order,
                &mut visited,
                &self.archetypes_index,
                &deps,
                arch_id,
            )
        }
    }

    fn clear(&mut self) {
        self.archetypes.clear();
        self.archetypes_index.clear();
        self.order.clear();
    }
}

impl Topo {
    /// Iterate a hierarchy in topological order from `root`
    pub fn new<T: ComponentValue>(relation: impl RelationExt<T>) -> Self {
        Self {
            relation: relation.id(),
            state: Default::default(),
        }
    }
}

impl<'w, Q, F> QueryStrategy<'w, Q, F> for Topo
where
    Q: 'w + Fetch<'w>,
    F: 'w + Fetch<'w>,
{
    type Borrow = TopoBorrow<'w, Q, F>;

    fn borrow(
        &'w mut self,
        query_state: super::borrow::QueryBorrowState<'w, Q, F>,
        dirty: bool,
    ) -> Self::Borrow {
        if dirty {
            self.state
                .update(self.relation, query_state.world, query_state.fetch);
        }

        TopoBorrow {
            topo: &self.state,
            state: query_state,
            prepared: Default::default(),
        }
    }

    fn access(
        &self,
        world: &'w crate::World,
        fetch: &'w crate::filter::Filtered<Q, F>,
    ) -> Vec<crate::Access> {
        let mut state = State::default();
        state.update(self.relation, world, fetch);

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

/// Borrowed state for [`Topo`] strategy
pub struct TopoBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    topo: &'w State,
    state: QueryBorrowState<'w, Q, F>,
    /// Archetypes are in topological order
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 8]>,
}

impl<'w, Q, F> TopoBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> TopoIter<'w, 'q, Q, F> {
        if self.prepared.is_empty() {
            self.prepared = self
                .topo
                .order
                .iter()
                .flat_map(|&idx| {
                    let arch_id = self.topo.archetypes[idx];
                    let arch = self.state.world.archetypes.get(arch_id);

                    self.state.prepare_fetch(arch_id, arch)
                })
                .collect();
        }

        TopoIter {
            iter: BatchedIter::new(self.prepared.iter_mut()).flatten(),
        }
    }
}

/// Iterates a hierarchy in topological order.
pub struct TopoIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    iter: Flatten<BatchedIter<'w, 'q, Q, F>>,
}

impl<'w, 'q, Q, F> Iterator for TopoIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    'w: 'q,
{
    type Item = <Q::Prepared as PreparedFetch<'q>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

#[cfg(test)]
mod test {
    use alloc::vec;
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{child_of, component_info, name, FetchExt, Query, World};
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn topological_sort() {
        let mut world = World::new();
        let [a, b, c, d, e, f, g] = *('a'..='g')
            .map(|i| {
                Entity::builder()
                    .set(name(), i.to_string())
                    .spawn(&mut world)
            })
            .collect_vec() else {unreachable!()};

        // Intentionally scrambled order as alphabetical order causes the input to already be
        // sorted.
        /*
         *    a     d
         *   / \   /
         *  g    f
         *  | \
         *  |  c
         *  | /
         *  e,b
         *
         *
         *  a,d
         *  g
         *  f
         *  c
         *  e,b
         */

        world.set(e, child_of(g), ()).unwrap();
        world.set(e, child_of(c), ()).unwrap();
        world.set(b, child_of(g), ()).unwrap();
        world.set(b, child_of(c), ()).unwrap();

        world.set(g, child_of(a), ()).unwrap();

        world.set(c, child_of(g), ()).unwrap();

        world.set(f, child_of(a), ()).unwrap();
        world.set(f, child_of(d), ()).unwrap();

        let mut state = State::default();

        let fetch = name().with() & !component_info().with();

        state.update(child_of.id(), &world, &fetch);

        let visited = state
            .order
            .iter()
            .map(|&idx| {
                let arch_id = state.archetypes[idx];
                let arch = world.archetypes.get(arch_id);

                arch.entities().to_vec()
            })
            .collect_vec();

        assert_eq!(
            visited,
            [vec![a, d], vec![g], vec![f], vec![c], vec![], vec![e, b]]
        );
    }

    #[test]
    fn topo_query() {
        component! {
            tree: (),
        }

        let mut world = World::new();

        let [_a, b, c, d, e, f, g] = *('a'..='g')
            .map(|i| {
                Entity::builder()
                    .set(name(), i.to_string())
                    .tag(tree())
                    .spawn(&mut world)
            })
            .collect_vec() else {unreachable!()};

        //   d ----*     a
        //   |     |
        //   |     b-----c
        //   |    / \    |
        //   |   /   *---f
        //   e -*
        //   |
        //   g

        world.set(b, child_of(d), ()).unwrap();

        world.set(e, child_of(d), ()).unwrap();
        world.set(e, child_of(b), ()).unwrap();

        world.set(c, child_of(b), ()).unwrap();

        world.set(f, child_of(b), ()).unwrap();
        world.set(f, child_of(c), ()).unwrap();

        world.set(g, child_of(e), ()).unwrap();

        let mut query = Query::new(name().cloned())
            .with_strategy(Topo::new(child_of))
            .without(component_info())
            .with(tree());

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, ["a", "d", "b", "c", "f", "e", "g"]);

        // Detaching `b` creates a separate tree
        //   d ----*     a
        //   |     |
        //   |     b     c
        //   |           |
        //   |           f
        //   e
        //   |
        //   g
        world.detach(b);

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, ["a", "d", "c", "f", "b", "e", "g"]);

        // Removing the `tree` from `e` is equivalent to removing the dependency
        world.remove(e, tree()).unwrap();

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, ["a", "d", "c", "f", "b", "g"]);
    }
}
