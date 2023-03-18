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

type AdjMap = BTreeMap<Entity, SmallVec<[usize; 8]>>;

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
                    btree_map::Entry::Occupied(_) => panic!("Cycle"),
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
    ///
    /// The passed function is invoked for each visited entity.
    ///
    /// The relation edge is supplied for visited children.
    pub fn traverse<V, Fn>(&mut self, value: &V, mut func: Fn)
    where
        Fn: for<'q> FnMut(<Q as FetchItem<'q>>::Item, Option<&T>, &V) -> V,
    {
        self.prepared.clear();
        // Safety: the iterator will not borrow this archetype again
        let loc = self.state.world.location(self.dfs.root).unwrap();
        let _arch_index = *self.dfs.state.archetypes_index.get(&loc.arch_id).unwrap();
        let arch = self.state.world.archetypes.get(loc.arch_id);

        // Fetch will never change and all calls are disjoint
        let mut p = self.state.prepare_fetch(loc.arch_id, arch).unwrap();

        let root = p.manual_chunk(Slice::single(loc.slot));

        if let Some(root) = root {
            self.traverse_inner(root, None, value, &mut func);
        }
    }

    fn traverse_inner<V, Fn>(
        &self,
        mut batch: Batch<Q::Prepared, F::Prepared>,
        parent: Option<&[T]>,
        value: &V,
        func: &mut Fn,
    ) where
        Q: 'w,
        F: 'w,
        Fn: for<'q> FnMut(<Q as FetchItem<'q>>::Item, Option<&T>, &V) -> V,
    {
        let d_state = &self.dfs.state;
        while let Some((slot, id, item)) = batch.next_full() {
            let value = (func)(item, parent.map(|v| &v[slot]), value);

            // Iterate the archetypes which contain all references to `id`
            for &arch_index in d_state.adj.get(&id).into_iter().flatten() {
                let arch_id = d_state.archetypes[arch_index];
                let arch = self.state.world.archetypes.get(arch_id);

                let parent = arch.borrow::<T>(ComponentKey::new(id, Some(self.dfs.relation)));

                let mut p = self.state.prepare_fetch(arch_id, arch).unwrap();

                let chunks = p.chunks();
                for batch in chunks {
                    self.traverse_inner(batch, parent.as_deref(), &value, func);
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
    archetypes: &'q mut [PreparedArchetype<'w, Q::Prepared, F::Prepared>],
    stack: SmallVec<[Batch<'q, Q::Prepared, F::Prepared>; 8]>,

    adj: &'q AdjMap,
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

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use crate::{child_of, entity_ids, name, CommandBuffer, FetchExt, Query};

    use super::*;

    #[test]
    fn dfs() {
        component! {
            tree: (),
        }

        let mut world = World::new();

        let [a, b, c, d, e, f, g] = *('a'..='g')
            .map(|i| {
                Entity::builder()
                    .set(name(), i.to_string())
                    .tag(tree())
                    .spawn(&mut world)
            })
            .collect_vec() else { unreachable!() };

        //       c
        //       |
        // *-----*-----*
        // |     |     |
        // b     d     e
        // |
        // *-----*
        // |     |
        // a     g
        // |
        // f

        world.set(a, child_of(b), ()).unwrap();
        world.set(b, child_of(c), ()).unwrap();
        world.set(d, child_of(c), ()).unwrap();
        world.set(e, child_of(c), ()).unwrap();
        world.set(f, child_of(a), ()).unwrap();
        world.set(g, child_of(b), ()).unwrap();

        // world.remove(tree, b());
    }

    #[test]
    fn traverse_dfs() {
        let mut world = World::new();
        use alloc::string::String;
        use alloc::string::ToString;

        component! {
            a: i32,
            path: String,
            b: &'static str,
        }

        let root = Entity::builder()
            .set(name(), "root".into())
            .set(a(), 0)
            .attach(
                child_of,
                Entity::builder()
                    .set(name(), "child.1".into())
                    .set(a(), 1)
                    .attach(
                        child_of,
                        Entity::builder()
                            .set(name(), "child.1.1".into())
                            .set(a(), 2),
                    ),
            )
            .attach(
                child_of,
                Entity::builder()
                    .set(name(), "child.2".into())
                    .set(a(), 3)
                    .set(b(), "Foo"),
            )
            .attach(
                child_of,
                Entity::builder().set(name(), "child.3".into()).set(a(), 4),
            )
            .spawn(&mut world);

        // let mut query = crate::Query::new((name().cloned(), a().copied()));
        let mut query =
            Query::new((name().cloned(), a().copied())).with_strategy(Dfs::new(child_of, root));

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(
            items,
            [
                ("root".to_string(), 0),
                ("child.1".to_string(), 1),
                ("child.1.1".to_string(), 2),
                ("child.3".to_string(), 4),
                ("child.2".to_string(), 3),
            ]
        );

        let mut cmd = CommandBuffer::new();

        eprintln!("Traversing");
        Query::new((entity_ids(), name()))
            .with_strategy(Dfs::new(child_of, root))
            .borrow(&world)
            .traverse(&Vec::new(), |(id, name), _, prefix| {
                eprintln!("Visited: {id}");
                let mut p = prefix.clone();
                p.push(name.clone());

                cmd.set(id, path(), p.join("::"));
                p
            });

        eprintln!("------------");

        cmd.apply(&mut world).unwrap();

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(
            items,
            [
                ("root".to_string(), 0),
                ("child.1".to_string(), 1),
                ("child.1.1".to_string(), 2),
                ("child.3".to_string(), 4),
                ("child.2".to_string(), 3),
            ]
        );

        let mut paths = Query::new(path().cloned()).collect_vec(&world);
        paths.sort();

        assert_eq!(
            paths,
            [
                "root",
                "root::child.1",
                "root::child.1::child.1.1",
                "root::child.2",
                "root::child.3",
            ]
        );

        // Change detection

        let mut query = Query::new((name().cloned(), a().modified().copied()))
            .with_strategy(Dfs::new(child_of, root));

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(
            items,
            [
                ("root".to_string(), 0),
                ("child.1".to_string(), 1),
                ("child.1.1".to_string(), 2),
                ("child.3".to_string(), 4),
                ("child.2".to_string(), 3),
            ]
        );

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, []);
        *world.get_mut(root, a()).unwrap() -= 1;
        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, [("root".to_string(), -1)]);

        Query::new((child_of(root), a().as_mut()))
            .borrow(&world)
            .for_each(|(_, a)| {
                *a *= -1;
            });

        // No changes, since the root is not modified
        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, []);

        Query::new((name(), a().as_mut()))
            .filter(child_of(root).with() | name().eq("root".to_string()))
            .borrow(&world)
            .for_each(|(_, a)| {
                *a *= -10;
            });

        let items = query.borrow(&world).iter().collect_vec();
        assert_eq!(
            items,
            [
                ("root".to_string(), 10),
                ("child.1".to_string(), 10),
                ("child.3".to_string(), 40),
                ("child.2".to_string(), 30),
            ]
        );
    }
}
