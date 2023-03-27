use core::marker::PhantomData;

use crate::{
    fetch::FetchAccessData, filter::Filtered, Access, AccessKind, ArchetypeId, ComponentValue,
};
use alloc::{collections::BTreeMap, vec::Vec};
use smallvec::SmallVec;

use crate::{Entity, Fetch, RelationExt, World};

use super::{borrow::QueryBorrowState, dfs::DfsIter, PreparedArchetype, QueryStrategy};

/// Traverse from all roots in depth first order
pub struct DfsRoots<T> {
    relation: Entity,

    state: State,

    marker: PhantomData<T>,
}

impl<T: ComponentValue> DfsRoots<T> {
    /// Iterate all hierarchies in depth-first order
    pub fn new(relation: impl RelationExt<T>) -> Self {
        Self {
            relation: relation.id(),

            state: Default::default(),
            marker: PhantomData,
        }
    }
}

impl<'w, Q, F, T: ComponentValue> QueryStrategy<'w, Q, F> for DfsRoots<T>
where
    Q: 'w + Fetch<'w>,
    F: 'w + Fetch<'w>,
{
    type Borrow = DfsRootsBorrow<'w, T, Q, F>;

    fn borrow(&'w mut self, query_state: QueryBorrowState<'w, Q, F>, dirty: bool) -> Self::Borrow {
        if dirty {
            self.state
                .update(query_state.world, self.relation, query_state.fetch)
        }

        DfsRootsBorrow::new(query_state, self)
    }

    fn access(&self, world: &'w World, fetch: &'w Filtered<Q, F>) -> Vec<crate::Access> {
        let mut state = State::default();
        state.update(world, self.relation, fetch);

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
#[derive(Default, Debug)]
struct State {
    /// Maps each entity to a list of indices of query archetypes
    edges: BTreeMap<Entity, SmallVec<[usize; 8]>>,
    archetypes: Vec<ArchetypeId>,
    archetypes_index: BTreeMap<ArchetypeId, usize>,
    roots: Vec<usize>,
}

impl State {
    pub(crate) fn update<'w, Q>(&mut self, world: &'w World, relation: Entity, fetch: &Q)
    where
        Q: Fetch<'w>,
    {
        self.edges.clear();
        self.archetypes.clear();
        self.archetypes_index.clear();
        self.roots.clear();

        for (arch_id, arch) in world.archetypes.iter() {
            if !fetch.filter_arch(arch) {
                continue;
            }

            let index = self.archetypes.len();
            self.archetypes.push(arch_id);
            assert!(self.archetypes_index.insert(arch_id, index).is_none());

            // Go backwards through the relations
            let mut root = true;
            for (key, _) in arch.relations_like(relation) {
                root = false;
                let object = key.object.unwrap();

                self.edges.entry(object).or_default().push(index);
            }

            if root {
                self.roots.push(index);
            }
        }
    }
}

/// Borrowed state for [`Dfs`] strategy
pub struct DfsRootsBorrow<'w, T, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 8]>,
    dfs: &'w DfsRoots<T>,
}

impl<'w, T, Q, F> DfsRootsBorrow<'w, T, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    T: ComponentValue,
{
    fn new(query_state: QueryBorrowState<'w, Q, F>, dfs: &'w DfsRoots<T>) -> Self {
        let prepared = dfs
            .state
            .archetypes
            .iter()
            .map(|&arch_id| {
                let arch = query_state.world.archetypes.get(arch_id);
                query_state.prepare_fetch(arch_id, arch).unwrap()
            })
            .collect();

        Self { prepared, dfs }
    }

    /// Iterate all items matched by query and filter.
    pub fn iter<'q>(&'q mut self) -> DfsIter<'w, 'q, Q, F>
    where
        'w: 'q,
    {
        // Safety: the iterator will not borrow these archetypes again
        let stack = self
            .dfs
            .state
            .roots
            .iter()
            .flat_map(|&arch_index| {
                let arch = &mut self.prepared[arch_index];
                // Fetch will never change and all calls are disjoint
                let p = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };
                p.chunks()
            })
            .collect();

        DfsIter {
            archetypes: &mut self.prepared[..],
            stack,
            adj: &self.dfs.state.edges,
        }
    }
}

#[cfg(test)]
mod test {
    use alloc::collections::BTreeSet;
    use itertools::Itertools;

    use crate::{child_of, entity_ids, name, Error, Query};

    use super::*;

    #[test]
    fn dfs() {
        component! {
            tree: (),
            other: (),
        }

        let mut world = World::new();
        let mut all = BTreeSet::new();

        let [a, b, c, d, e, f, g, h, i, j, k, _l] = *('a'..='l')
            .map(|i| {
                let id = Entity::builder()
                    .set(name(), i.into())
                    .tag(tree())
                    .spawn(&mut world);

                    all.insert(id);
                id
            })
            .collect_vec() else { unreachable!() };

        world.set(i, other(), ()).unwrap();

        //       c              i         l
        //       |              |
        // *-----*-----*    *---*---*
        // |     |     |    |   |   |
        // b     d     e    h   k   j
        // |
        // *-----*
        // |     |
        // a     g
        // |
        // f

        let mut adj = BTreeMap::from([
            (b, c),
            (d, c),
            (e, c),
            //
            (a, b),
            (g, b),
            //
            (f, a),
            //
            (h, i),
            (k, i),
            (j, i),
        ]);

        from_edges(&mut world, &adj).unwrap();

        eprintln!("World: {world:#?}");

        let mut query = Query::new((entity_ids(), name())).with_strategy(DfsRoots::new(child_of));

        assert_dfs(query.borrow(&world).iter(), &adj, &all);

        world.set(b, child_of(h), ()).unwrap();

        adj.insert(b, h);

        assert_dfs(query.borrow(&world).iter(), &adj, &all);
    }

    fn from_edges<'a>(
        world: &mut World,
        iter: impl IntoIterator<Item = (&'a Entity, &'a Entity)>,
    ) -> Result<(), Error> {
        for (&child, &parent) in iter {
            world.set(child, child_of(parent), ())?;
        }
        Ok(())
    }

    fn assert_dfs<'a>(
        iter: impl Iterator<Item = (Entity, &'a String)>,
        edges: &BTreeMap<Entity, Entity>,
        all: &BTreeSet<Entity>,
    ) {
        let mut visited = BTreeSet::new();

        for (id, name) in iter {
            if let Some(parent) = edges.get(&id) {
                assert!(
                    visited.contains(parent),
                    "Child {name} visited before parent"
                );
            }

            assert!(visited.insert(id));
        }

        assert_eq!(all, &visited, "Not all visited");
    }
}
