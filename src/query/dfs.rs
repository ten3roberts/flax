use core::marker::PhantomData;

use crate::{
    archetype::{ArchetypeId, Slice},
    component::{ComponentKey, ComponentValue},
    fetch::{FetchAccessData, PreparedFetch},
    filter::{All, Filtered},
    relation::RelationExt,
    system::{Access, AccessKind},
    FetchItem,
};
use alloc::{collections::BTreeMap, vec::Vec};
use smallvec::SmallVec;

use crate::{Entity, Fetch, World};

use super::{borrow::QueryBorrowState, Chunk, PreparedArchetype, QueryStrategy};

type AdjMap = BTreeMap<Entity, SmallVec<[usize; 8]>>;

/// Traverse from all roots in depth first order
pub struct Dfs<T> {
    relation: Entity,

    state: State,

    marker: PhantomData<T>,
}

impl<T: ComponentValue> Dfs<T> {
    /// Iterate all hierarchies in depth-first order
    pub fn new(relation: impl RelationExt<T>) -> Self {
        Self {
            relation: relation.id(),

            state: Default::default(),
            marker: PhantomData,
        }
    }
}

impl<'w, Q, F, T: ComponentValue> QueryStrategy<'w, Q, F> for Dfs<T>
where
    Q: 'w + Fetch<'w>,
    F: 'w + Fetch<'w>,
{
    type Borrow = DfsBorrow<'w, Q, F, T>;

    fn borrow(&'w mut self, query_state: QueryBorrowState<'w, Q, F>, dirty: bool) -> Self::Borrow {
        if dirty {
            self.state
                .update(query_state.world, self.relation, query_state.fetch)
        }

        DfsBorrow::new(query_state, self)
    }

    fn access(&self, world: &'w World, fetch: &'w Filtered<Q, F>, dst: &mut Vec<Access>) {
        let mut state = State::default();
        state.update(world, self.relation, fetch);

        state.archetypes.iter().for_each(|&arch_id| {
            let arch = world.archetypes.get(arch_id);
            let data = FetchAccessData {
                world,
                arch,
                arch_id,
            };

            fetch.access(data, dst);
        });

        dst.push(Access {
            kind: AccessKind::World,
            mutable: false,
        });
    }
}
#[derive(Default, Debug)]
struct State {
    /// Maps each entity to a list of indices of query archetypes
    edges: AdjMap,
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
            if !fetch.filter_arch(FetchAccessData {
                world,
                arch,
                arch_id,
            }) {
                continue;
            }

            let index = self.archetypes.len();
            self.archetypes.push(arch_id);
            assert!(self.archetypes_index.insert(arch_id, index).is_none());

            // Go backwards through the relations
            let mut root = true;
            for (key, _) in arch.relations_like(relation) {
                root = false;
                let target = key.target.unwrap();

                self.edges.entry(target).or_default().push(index);
            }

            if root {
                self.roots.push(index);
            }
        }
    }
}

/// Borrowed state for [`Dfs`] strategy
pub struct DfsBorrow<'w, Q, F = All, T = ()>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 8]>,
    query_state: QueryBorrowState<'w, Q, F>,
    dfs: &'w Dfs<T>,
}

impl<'w, Q, F, T> DfsBorrow<'w, Q, F, T>
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
            dfs,
            query_state,
        }
    }

    /// Iterate the subtree of `root` in depth first order.
    ///
    /// Returns an empty iterator if `root` is not valid
    pub fn iter_from<'q>(&'q mut self, root: Entity) -> DfsIter<'w, 'q, Q, F>
    where
        'w: 'q,
    {
        let mut iter = DfsIter {
            prepared: &mut self.prepared[..],
            stack: smallvec::smallvec![],
            adj: &self.dfs.state.edges,
        };

        let loc = self.query_state.world.location(root);
        if let Ok(loc) = loc {
            let arch_index = *self.dfs.state.archetypes_index.get(&loc.arch_id).unwrap();
            // Safety: is root archetype
            unsafe {
                iter.push_slice_to_stack(arch_index, Slice::single(loc.slot));
            }
        }

        iter

        // let arch = &mut self.prepared[arch_index];
        // // Fetch will never change and all calls are disjoint
        // let p = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };
        // if let Some(v) = p.manual_chunk(Slice::single(loc.slot)) {

        // }
    }

    /// Iterate all trees in depth first order
    pub fn iter<'q>(&'q mut self) -> DfsIter<'w, 'q, Q, F>
    where
        'w: 'q,
    {
        let mut iter = DfsIter {
            prepared: &mut self.prepared[..],
            stack: smallvec::smallvec![],
            adj: &self.dfs.state.edges,
        };

        // Safety: the iterator will not borrow these archetypes again
        for &arch_index in &self.dfs.state.roots {
            unsafe { iter.push_to_stack(arch_index) }
            // let arch = &mut prepared[arch_index];
            // // Fetch will never change and all calls are disjoint
            // let p = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };
            // p.chunks()
        }

        iter
    }

    /// Traverse the subtree recursively, visiting each node using the provided function
    /// `visit(query, edge, value)` where `value` is the return value of the visit.
    pub fn traverse_from<V, Visit>(&mut self, root: Entity, value: &V, mut visit: Visit)
    where
        Visit: for<'q> FnMut(<Q as FetchItem<'q>>::Item, Option<&T>, &V) -> V,
    {
        let Ok(loc) = self.query_state.world.location(root) else {
            return;
        };

        let dfs = &self.dfs;
        let prepared = (&mut self.prepared[..]) as *mut [_] as *mut PreparedArchetype<_, _>;
        let arch_index = *dfs.state.archetypes_index.get(&loc.arch_id).unwrap();

        // Fetch will never change and all calls are disjoint as the graph is acyclic
        let p = unsafe { &mut *prepared.add(arch_index) };

        if let Some(mut chunk) = unsafe { p.create_chunk(Slice::single(loc.slot)) } {
            Self::traverse_batch(
                self.query_state.world,
                dfs,
                prepared,
                &mut chunk,
                None,
                value,
                &mut visit,
            )
        }
    }

    /// Traverse all trees recursively, visiting each node using the provided function
    /// `visit(query, edge, value)` where `value` is the return value of the parent.
    pub fn traverse<V, Visit>(&mut self, value: &V, mut visit: Visit)
    where
        Visit: for<'q> FnMut(<Q as FetchItem<'q>>::Item, Option<&T>, &V) -> V,
    {
        let dfs = &self.dfs;
        let prepared = (&mut self.prepared[..]) as *mut [_] as *mut PreparedArchetype<_, _>;
        for &arch_index in dfs.state.roots.iter() {
            // Fetch will never change and all calls are disjoint
            let p = unsafe { &mut *prepared.add(arch_index) };
            for mut chunk in p.chunks() {
                Self::traverse_batch(
                    self.query_state.world,
                    dfs,
                    prepared,
                    &mut chunk,
                    None,
                    value,
                    &mut visit,
                )
            }
        }
    }

    fn traverse_batch<V, Visit>(
        world: &World,
        dfs: &Dfs<T>,
        // Uses a raw pointer to be able to recurse inside the loop
        // Alternative: release all borrows and borrow/prepare each fetch inside the loop
        prepared: *mut PreparedArchetype<Q::Prepared, F::Prepared>,
        chunk: &mut Chunk<Q::Prepared>,
        edge: Option<&[T]>,
        value: &V,
        visit: &mut Visit,
    ) where
        Visit: for<'q> FnMut(<Q as FetchItem<'q>>::Item, Option<&T>, &V) -> V,
        Q: 'w,
        F: 'w,
    {
        while let Some((slot, id, item)) = chunk.next_full() {
            let value = (visit)(item, edge.map(|v| &v[slot]), value);

            // Iterate the archetypes which contain all references to `id`
            for &arch_index in dfs.state.edges.get(&id).into_iter().flatten() {
                let arch_id = dfs.state.archetypes[arch_index];
                let arch = world.archetypes.get(arch_id);

                let edge = arch.borrow::<T>(ComponentKey::new(dfs.relation, Some(id)));

                let p = unsafe { &mut *prepared.add(arch_index) };

                for mut chunk in p.chunks() {
                    Self::traverse_batch(
                        world,
                        dfs,
                        prepared,
                        &mut chunk,
                        edge.as_ref().map(|v| v.get()),
                        &value,
                        visit,
                    )
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
    pub(crate) prepared: &'q mut [PreparedArchetype<'w, Q::Prepared, F::Prepared>],
    pub(crate) stack: SmallVec<[Chunk<'q, Q::Prepared>; 8]>,

    pub(crate) adj: &'q AdjMap,
}

impl<'w, 'q, Q, F> DfsIter<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Pushes all chunks from arch onto the stack
    ///
    /// # Safety
    /// The arch_index must not be pushed twice or appear later in the stack as a result of
    /// the hierarchy
    unsafe fn push_to_stack(&mut self, arch_index: usize) {
        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let p = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };
        self.stack.extend(p.chunks())
    }

    /// See: [`Self::push_to_stack`]
    unsafe fn push_slice_to_stack(&mut self, arch_index: usize, slice: Slice) {
        let arch = &mut self.prepared[arch_index];
        // Fetch will never change and all calls are disjoint
        let p = unsafe { &mut *(arch as *mut PreparedArchetype<_, _>) };
        if let Some(chunk) = p.create_chunk(slice) {
            self.stack.push(chunk)
        }
    }
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
                    let p = &mut self.prepared[arch_index];

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
    use alloc::collections::BTreeSet;
    use itertools::Itertools;

    use crate::{
        components::{child_of, name},
        entity_ids, CommandBuffer, Error, FetchExt, Query,
    };

    use super::*;

    #[test]
    fn dfs_cycle() {
        component! {
            tree: (),
        }

        let mut world = World::new();

        let [a, b, c] = *('a'..='c')
            .map(|i| {
                Entity::builder()
                    .set(name(), i.into())
                    .tag(tree())
                    .spawn(&mut world)
            })
            .collect_vec()
        else {
            unreachable!()
        };

        world.set(b, child_of(a), ()).unwrap();
        world.set(c, child_of(b), ()).unwrap();

        let mut query = Query::new(entity_ids()).with_strategy(Dfs::new(child_of));
        assert_eq!(query.borrow(&world).iter().collect_vec(), [a, b, c]);

        world.set(a, child_of(c), ()).unwrap();
        assert_eq!(query.borrow(&world).iter().collect_vec(), []);
    }

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
            .collect_vec()
        else {
            unreachable!()
        };

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

        let mut edges = BTreeMap::from([
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

        from_edges(&mut world, &edges).unwrap();

        let mut query = Query::new((entity_ids(), name())).with_strategy(Dfs::new(child_of));

        assert_dfs(query.borrow(&world).iter(), &edges, &all);

        world.set(b, child_of(h), ()).unwrap();

        edges.insert(b, h);

        assert_dfs(query.borrow(&world).iter(), &edges, &all);

        assert_dfs(
            query.borrow(&world).iter_from(c),
            &edges,
            &[c, d, e].into_iter().collect(),
        );
    }

    #[test]
    fn traverse_dfs() {
        let mut world = World::new();
        use alloc::string::String;
        use alloc::string::ToString;

        component! {
            a: i32,
            path: String,
        }

        let ids = ('a'..='e')
            .zip(0..)
            .map(|(v, i)| {
                Entity::builder()
                    .set(name(), v.into())
                    .set(a(), i)
                    .spawn(&mut world)
            })
            .collect_vec();

        let all = BTreeSet::from_iter(ids.iter().copied());

        let items = Query::new((name().cloned(), a().modified().copied())).collect_vec(&world);

        assert_eq!(
            items,
            [
                ("a".to_string(), 0),
                ("b".to_string(), 1),
                ("c".to_string(), 2),
                ("d".to_string(), 3),
                ("e".to_string(), 4),
            ]
        );

        let edges = BTreeMap::from([
            (ids[1], ids[0]),
            (ids[2], ids[1]),
            (ids[3], ids[0]),
            (ids[4], ids[0]),
        ]);

        from_edges(&mut world, &edges).unwrap();

        let items = Query::new((name().cloned(), a().modified().copied()))
            .borrow(&world)
            .iter()
            .sorted()
            .collect_vec();

        assert_eq!(
            items,
            [
                ("a".to_string(), 0),
                ("b".to_string(), 1),
                ("c".to_string(), 2),
                ("d".to_string(), 3),
                ("e".to_string(), 4),
            ]
        );
        // let mut query = crate::Query::new((name().cloned(), a().copied()));
        let mut query = Query::new((entity_ids(), a().copied())).with_strategy(Dfs::new(child_of));

        assert_dfs(query.borrow(&world).iter(), &edges, &all);

        let mut cmd = CommandBuffer::new();

        Query::new((entity_ids(), name()))
            .with_strategy(Dfs::new(child_of))
            .borrow(&world)
            .traverse(&Vec::new(), |(id, name), _, prefix| {
                let mut p = prefix.clone();
                p.push(name.clone());

                cmd.set(id, path(), p.join("::"));
                p
            });

        cmd.apply(&mut world).unwrap();

        assert_dfs(query.borrow(&world).iter(), &edges, &all);
        // assert_eq!(
        //     items,
        //     [
        //         ("root".to_string(), 0),
        //         ("child.1".to_string(), 1),
        //         ("child.1.1".to_string(), 2),
        //         ("child.3".to_string(), 4),
        //         ("child.2".to_string(), 3),
        //     ]
        // );

        let paths = Query::new(path().cloned())
            .borrow(&world)
            .iter()
            .sorted()
            .collect_vec();

        assert_eq!(paths, ["a", "a::b", "a::b::c", "a::d", "a::e",]);

        // Change detection

        let mut query = Query::new((name().cloned(), a().modified().copied()))
            .with_strategy(Dfs::new(child_of));

        let items = query.borrow(&world).iter().sorted().collect_vec();

        assert_eq!(
            items,
            [
                ("a".to_string(), 0),
                ("b".to_string(), 1),
                ("c".to_string(), 2),
                ("d".to_string(), 3),
                ("e".to_string(), 4),
            ]
        );

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, []);
        *world.get_mut(ids[0], a()).unwrap() -= 1;
        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, [("a".to_string(), -1)]);

        Query::new((child_of(ids[0]), a().as_mut()))
            .borrow(&world)
            .for_each(|(_, a)| {
                *a *= -1;
            });

        // No changes, since the root is not modified
        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(items, []);

        Query::new((name(), a().as_mut()))
            .filter(child_of(ids[0]).with() | name().eq("a".to_string()))
            .borrow(&world)
            .for_each(|(_, a)| {
                *a *= -10;
            });

        let items = query.borrow(&world).iter().sorted().collect_vec();
        assert_eq!(
            items,
            [
                ("a".to_string(), 10),
                ("b".to_string(), 10),
                ("d".to_string(), 30),
                // c is a too deep
                ("e".to_string(), 40),
            ]
        );
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

    fn assert_dfs<T>(
        iter: impl Iterator<Item = (Entity, T)>,
        edges: &BTreeMap<Entity, Entity>,
        all: &BTreeSet<Entity>,
    ) {
        let mut visited = BTreeSet::new();

        for (id, _) in iter {
            if let Some(parent) = edges.get(&id) {
                assert!(visited.contains(parent), "Child {id} visited before parent");
            }

            assert!(visited.insert(id));
        }

        assert_eq!(&visited, all, "Not all visited");
    }
}
