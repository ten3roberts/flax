use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    component::ComponentValue,
    fetch::FetchAccessData,
    filter::{All, And, Filtered},
    Entity, Fetch, FetchItem, RelationExt, World,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use core::{fmt::Debug, iter::Zip, marker::PhantomData, ops::Range, slice::Iter};
use smallvec::SmallVec;

use super::{borrow::QueryBorrowState, PreparedArchetype};

/// Allows random traversal of a graph formed by a relation
pub struct GraphQuery<Q, F = All> {
    relation: Entity,
    fetch: Filtered<Q, F>,

    change_tick: u32,
    archetype_gen: u32,
    state: GraphState,
}

impl<Q> GraphQuery<Q, All> {
    /// Creates a new [`GraphQuery`]
    pub fn new<T, R>(relation: R, fetch: Q) -> Self
    where
        T: ComponentValue,
        R: RelationExt<T>,
    {
        Self {
            relation: relation.id(),
            fetch: Filtered::new(fetch, All, false),
            change_tick: 0,
            archetype_gen: 0,
            state: Default::default(),
        }
    }
}

impl<Q, F> GraphQuery<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    /// Adds a new filter to the query.
    /// This filter is and:ed with the existing filters.
    pub fn filter<G>(self, filter: G) -> GraphQuery<Q, And<F, G>> {
        GraphQuery {
            fetch: Filtered::new(
                self.fetch.fetch,
                And(self.fetch.filter, filter),
                self.fetch.include_components,
            ),
            relation: self.relation,
            change_tick: 0,
            archetype_gen: 0,
            state: Default::default(),
        }
    }

    /// Prepares the query upon the world.
    pub fn borrow<'w>(&'w mut self, world: &'w World) -> GraphBorrow<Q, F> {
        // The tick of the last iteration
        let mut old_tick = self.change_tick;

        let new_tick = if Q::MUTABLE {
            world.advance_change_tick();
            world.change_tick()
        } else {
            world.change_tick()
        };

        if new_tick < old_tick {
            old_tick = 0;
        }

        self.change_tick = new_tick;

        let query_state = QueryBorrowState {
            old_tick,
            new_tick,
            world,
            fetch: &self.fetch,
        };

        let archetype_gen = world.archetype_gen();
        let dirty = archetype_gen > self.archetype_gen;

        if dirty {
            self.state.update(world, self.relation, &self.fetch);
        }

        self.archetype_gen = archetype_gen;

        let prepared = self
            .state
            .archetypes
            .iter()
            .map(|&arch_id| {
                let arch = query_state.world.archetypes.get(arch_id);
                query_state.prepare_fetch(arch_id, arch).unwrap()
            })
            .collect();

        GraphBorrow {
            world,
            relation: self.relation,
            prepared,
            state: &self.state,
        }
    }
}

#[derive(Default, Debug)]
struct GraphState {
    /// Maps each entity to a list of indices of query archetypes
    edges: BTreeMap<Entity, SmallVec<[ArchetypeId; 8]>>,
    archetypes: Vec<ArchetypeId>,
    archetypes_index: BTreeMap<ArchetypeId, usize>,
}

impl GraphState {
    fn update<'w, Q>(&mut self, world: &'w World, relation: Entity, fetch: &Q)
    where
        Q: Fetch<'w>,
    {
        self.edges.clear();
        self.archetypes.clear();
        self.archetypes_index.clear();

        for (arch_id, arch) in world.archetypes.iter() {
            if fetch.filter_arch(FetchAccessData {
                world,
                arch,
                arch_id,
            }) {
                let index = self.archetypes.len();
                self.archetypes.push(arch_id);
                assert!(self.archetypes_index.insert(arch_id, index).is_none())
            }

            // Go backwards through the relations
            for (key, _) in arch.relations_like(relation) {
                let object = key.object.unwrap();

                self.edges.entry(object).or_default().push(arch_id);
            }
        }
    }
}

/// The borrowed state for a [`GraphQuery`]
pub struct GraphBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    world: &'w World,
    relation: Entity,
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 16]>,
    state: &'w GraphState,
}

impl<'w, Q: Debug, F: Debug> Debug for GraphBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GraphBorrow")
            .field("world", &self.world)
            .field("relation", &self.relation)
            .finish()
    }
}

impl<'w, Q, F> GraphBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Get the node in the graph for the entity.
    pub fn get(&self, id: Entity) -> Option<Node<'w, Q, F>> {
        let loc = self.world.location(id).ok()?;

        Some(Node {
            id,
            slot: loc.slot,
            arch_id: loc.arch_id,
            state: self.state,
            world: self.world,
            _marker: PhantomData,
        })
    }
}

/// A cursor to a node/entity in the graph
pub struct Node<'w, Q, F> {
    id: Entity,
    slot: Slot,
    arch_id: ArchetypeId,
    state: &'w GraphState,
    world: &'w World,
    _marker: PhantomData<(Q, F)>,
}

impl<'w, Q, F> Clone for Node<'w, Q, F> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            slot: self.slot,
            arch_id: self.arch_id,
            state: self.state,
            world: self.world,
            _marker: PhantomData,
        }
    }
}

impl<'w, Q, F> Debug for Node<'w, Q, F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Node").field("id", &self.id).finish()
    }
}

impl<'w, Q, F> Node<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Returns the fetch item at the current entity, if applicable.
    ///
    /// If the entity doesn't match, `None` is returned.
    pub fn get<'q>(
        &self,
        borrow: &'q mut GraphBorrow<'w, Q, F>,
    ) -> Option<<Q as FetchItem<'q>>::Item>
    where
        Q: Fetch<'w>,
        F: Fetch<'w>,
    {
        let index = *self.state.archetypes_index.get(&self.arch_id)?;

        let p = &mut borrow.prepared[index];

        // Safety: &mut borrow
        unsafe { p.create_chunk(Slice::single(self.slot))? }.next()
    }

    /// Traverse the immediate children of the current node.
    pub fn children(&self) -> Children<'w, Q, F> {
        let archetypes = self
            .state
            .edges
            .get(&self.id)
            .map(|v| v.as_slice())
            .unwrap_or_default()
            .iter();

        Children {
            archetypes,
            current: None,
            state: self.state,
            _marker: PhantomData,
            world: self.world,
        }
    }

    /// Traverse the current subtree including the current node in depth-first order.
    pub fn dfs(&self) -> DfsIter<'w, Q, F> {
        let stack = smallvec::smallvec![self.clone()];

        DfsIter {
            stack,
            visited: Default::default(),
        }
    }
}

type ArchetypeNodes<'a> = (ArchetypeId, Zip<Range<Slot>, Iter<'a, Entity>>);

#[derive(Debug)]
/// Iterates the immediate children of a node
pub struct Children<'w, Q, F> {
    archetypes: core::slice::Iter<'w, ArchetypeId>,
    current: Option<ArchetypeNodes<'w>>,
    state: &'w GraphState,
    world: &'w World,
    _marker: PhantomData<(Q, F)>,
}

impl<'w, Q, F> Iterator for Children<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    type Item = Node<'w, Q, F>;
    fn next(&mut self) -> Option<Node<'w, Q, F>> {
        loop {
            if let Some((arch_id, v)) = self.current.as_mut() {
                if let Some((slot, &id)) = v.next() {
                    return Some(Node {
                        id,
                        slot,
                        arch_id: *arch_id,
                        state: self.state,
                        world: self.world,
                        _marker: PhantomData,
                    });
                }
            }

            let arch_id = *self.archetypes.next()?;
            let arch = self.world.archetypes.get(arch_id);

            let ids = arch.slots().iter().zip(arch.entities());

            self.current = Some((arch_id, ids))
        }
    }
}

/// Iterate a hierarchy in depth-first order
pub struct DfsIter<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    stack: SmallVec<[Node<'w, Q, F>; 16]>,

    visited: BTreeSet<Entity>,
}

impl<'w, Q, F> Iterator for DfsIter<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    type Item = Node<'w, Q, F>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let node = self.stack.pop()?;
            if !self.visited.insert(node.id) {
                continue;
            }

            // Add the children
            self.stack.extend(node.children());
            return Some(node);
        }
    }
}

#[cfg(test)]
mod test {
    use alloc::{string::String, vec, vec::Vec};
    use itertools::Itertools;

    use crate::{
        components::{child_of, name},
        filter::With,
        Component,
    };

    use super::*;

    #[test]
    fn traverse_tree() {
        component! {
            a: i32,
            b: &'static str,
        }

        let mut world = World::new();

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
                    .set(b(), "Foo"),
            )
            .attach(
                child_of,
                Entity::builder().set(name(), "child.3".into()).set(a(), 4),
            )
            .spawn(&mut world);

        {
            let mut query = GraphQuery::new(child_of, name());

            let mut borrow = query.borrow(&world);

            let root = borrow.get(root).unwrap();

            assert_eq!(root.get(&mut borrow), Some(&"root".into()));

            let children = root
                .children()
                .flat_map(|v| v.get(&mut borrow).cloned())
                .sorted()
                .collect_vec();

            assert_eq!(children, ["child.1", "child.2", "child.3"]);
        }

        // Test with a filter
        {
            let mut query = GraphQuery::new(child_of, name()).filter(a().with());

            let mut borrow = query.borrow(&world);

            {
                let root = borrow.get(root).unwrap();

                assert_eq!(root.get(&mut borrow), Some(&"root".into()));

                let children = root
                    .children()
                    .map(|v| v.get(&mut borrow).cloned())
                    .sorted()
                    .collect_vec();

                assert_eq!(
                    children,
                    [None, Some("child.1".into()), Some("child.3".into())]
                );

                let mut paths: Vec<(Vec<Option<String>>, usize)> = Vec::new();

                fn traverse<'w>(
                    borrow: &mut GraphBorrow<'w, Component<String>, And<All, With>>,
                    node: Node<'w, Component<String>, And<All, With>>,
                    path: &[Option<String>],
                    paths: &mut Vec<(Vec<Option<String>>, usize)>,
                    depth: usize,
                ) {
                    let name = node.get(borrow).cloned();
                    let path = path.iter().cloned().chain([name]).collect_vec();
                    paths.push((path.clone(), depth));

                    for node in node.children() {
                        traverse(borrow, node, &path, paths, depth + 1)
                    }
                }

                traverse(&mut borrow, root, &[], &mut paths, 0);

                pretty_assertions::assert_eq!(
                    paths,
                    [
                        (vec![Some("root".into())], 0),
                        (vec![Some("root".into()), Some("child.1".into())], 1),
                        (
                            vec![
                                Some("root".into()),
                                Some("child.1".into()),
                                Some("child.1.1".into())
                            ],
                            2
                        ),
                        (vec![Some("root".into()), Some("child.3".into())], 1),
                        (vec![Some("root".into()), None], 1),
                    ],
                );
            }

            let root = borrow.get(root).unwrap();
            let items = root
                .dfs()
                .flat_map(|v| v.get(&mut borrow).cloned())
                .collect_vec();

            pretty_assertions::assert_eq!(items, ["root", "child.3", "child.1", "child.1.1"]);
        }
    }
}
