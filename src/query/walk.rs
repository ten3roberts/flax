use core::{
    iter::{Enumerate, Zip},
    marker::PhantomData,
    ops::Range,
    slice::Iter,
};

use alloc::collections::BTreeMap;
use itertools::Itertools;
use smallvec::SmallVec;

use crate::{
    archetype::{Slice, Slot},
    filter::Filtered,
    All, And, ArchetypeId, ComponentValue, Entity, Fetch, FetchItem, PreparedArchetype, Query,
    RelationExt, World,
};

use super::{borrow::QueryBorrowState, ArchetypeSearcher};

/// Allows randomly walking a hierarchy formed by a relation
pub struct TreeWalker<Q, F = All> {
    relation: Entity,
    fetch: Filtered<Q, F>,

    change_tick: u32,
    archetype_gen: u32,
    adj: GraphState,
}

impl<Q> TreeWalker<Q, All> {
    /// Creates a new [`TreeWalker`]
    pub fn new<T, R>(relation: R, fetch: Q) -> Self
    where
        T: ComponentValue,
        R: RelationExt<T>,
    {
        Self {
            relation: relation.id(),
            fetch: Filtered::new(fetch, All),
            change_tick: 0,
            archetype_gen: 0,
            adj: Default::default(),
        }
    }
}

impl<Q, F> TreeWalker<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    /// Adds a new filter to the walker.
    /// This filter is and:ed with the existing filters.
    pub fn filter<G>(self, filter: G) -> TreeWalker<Q, And<F, G>> {
        TreeWalker {
            fetch: Filtered::new(self.fetch.fetch, And::new(self.fetch.filter, filter)),
            relation: self.relation,
            change_tick: 0,
            archetype_gen: 0,
            adj: Default::default(),
        }
    }

    pub fn borrow<'w>(&'w mut self, world: &'w World) -> TreeWalkerBorrow<Q, F> {
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
            self.adj.update(world, self.relation, &self.fetch);
        }

        self.archetype_gen = archetype_gen;

        let prepared = self
            .adj
            .archetypes
            .iter()
            .map(|&arch_id| {
                let arch = query_state.world.archetypes.get(arch_id);
                query_state.prepare_fetch(arch_id, arch).unwrap()
            })
            .collect();

        TreeWalkerBorrow {
            world,
            relation: self.relation,
            prepared,
            adj: &self.adj,
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
    fn update<'w, Q, F>(&mut self, world: &'w World, relation: Entity, fetch: &Filtered<Q, F>)
    where
        Q: Fetch<'w>,
        F: Fetch<'w>,
    {
        self.edges.clear();
        self.archetypes.clear();
        self.archetypes_index.clear();

        for (arch_id, arch) in world.archetypes.iter() {
            if fetch.filter_arch(arch) {
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

pub struct TreeWalkerBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    world: &'w World,
    relation: Entity,
    prepared: SmallVec<[PreparedArchetype<'w, Q::Prepared, F::Prepared>; 16]>,
    adj: &'w GraphState,
}

impl<'w, Q, F> TreeWalkerBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Traverse the graph starting from `id`.
    pub fn root(&mut self, id: Entity) -> Option<TreeCursor<'w, '_, Q, F>> {
        let loc = self.world.location(id).ok()?;

        Some(TreeCursor {
            id,
            slot: loc.slot,
            arch_id: loc.arch_id,
            borrow: self,
        })
    }
}

impl<'w, Q: core::fmt::Debug, F: core::fmt::Debug> core::fmt::Debug for TreeWalkerBorrow<'w, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TreeWalkerBorrow")
            .field("world", &self.world)
            .field("relation", &self.relation)
            .field("adj", &self.adj)
            .finish()
    }
}

/// A cursor to a node/entity in the graph
pub struct TreeCursor<'w, 'q, Q, F = All>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    id: Entity,
    slot: Slot,
    arch_id: ArchetypeId,
    borrow: &'q mut TreeWalkerBorrow<'w, Q, F>,
}

impl<'q, 'w, Q, F> TreeCursor<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    // Returns the fetch item at the current entity, if applicable.
    //
    // If the entity doesn't match, `None` is returned.
    fn get(&mut self) -> Option<<Q as FetchItem<'_>>::Item> {
        let index = *self.borrow.adj.archetypes_index.get(&self.arch_id)?;

        let p = &mut self.borrow.prepared[index];

        p.manual_chunk(Slice::single(self.slot))?.next()
    }

    /// Traverse the immediate children of the current node.
    fn children(&mut self) -> Children<'w, '_, Q, F> {
        let archetypes = self
            .borrow
            .adj
            .edges
            .get(&self.id)
            .map(|v| v.as_slice())
            .unwrap_or_default()
            .iter();

        Children {
            archetypes,
            current: None,
            borrow: self.borrow,
        }
    }

    fn map_children<Func, T>(&mut self, func: Func) -> MapChildren<'w, '_, Q, F, Func>
    where
        Func: for<'x> FnMut(Option<<Q as FetchItem<'x>>::Item>) -> T,
    {
        MapChildren {
            children: self.children(),
            func,
        }
    }
}

type ArchetypeNodes<'a> = (ArchetypeId, Enumerate<Iter<'a, Entity>>);

#[derive(Debug)]
pub struct Children<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    archetypes: core::slice::Iter<'w, ArchetypeId>,
    current: Option<ArchetypeNodes<'w>>,

    borrow: &'q mut TreeWalkerBorrow<'w, Q, F>,
}

impl<'w, 'q, Q, F> Children<'w, 'q, Q, F>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    /// Returns the next child
    ///
    /// **Note**: this is not an iterator, as the borrowed state is lent to the returned item.
    ///
    /// This is needed as only one node at the time can access the fetch state.
    fn next_child(&mut self) -> Option<TreeCursor<'w, '_, Q, F>> {
        loop {
            if let Some((arch_id, v)) = self.current.as_mut() {
                if let Some((slot, &id)) = v.next() {
                    return Some(TreeCursor {
                        id,
                        slot,
                        arch_id: *arch_id,
                        borrow: self.borrow,
                    });
                }
            }

            let arch_id = *self.archetypes.next()?;
            let arch = self.borrow.world.archetypes.get(arch_id);

            let ids = arch.entities().iter().enumerate();

            self.current = Some((arch_id, ids))
        }
    }
}

/// Adapter for children traversal which exposes an iterator interface.
///
/// This is done by a mapping function from the temporary *lending* style iterator to a longer
/// lifetime.
///
///
/// This is less flexible than manually traversing the hierarchy. Most notably is does not allow
/// the user to control or stop the iterator.
#[derive(Debug)]
pub struct MapChildren<'w, 'q, Q, F, Func>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
{
    children: Children<'w, 'q, Q, F>,
    func: Func,
}

impl<'w, 'q, Q, F, Func, T> Iterator for MapChildren<'w, 'q, Q, F, Func>
where
    Q: Fetch<'w>,
    F: Fetch<'w>,
    Func: for<'x> FnMut(Option<<Q as FetchItem<'x>>::Item>) -> T,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let mut node = self.children.next_child()?;

        Some((self.func)(node.get()))
    }
}

#[cfg(test)]
mod test {
    use crate::{child_of, name, Component, With};

    use super::*;

    #[test]
    fn traverse_tree() {
        component! {
            a: i32,
            path: String,
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
            let mut walker = TreeWalker::new(child_of, name());

            let mut borrow = walker.borrow(&world);

            let mut root = borrow.root(root).unwrap();

            assert_eq!(root.get(), Some(&"root".into()));

            let children = root
                .map_children(|v| v.cloned())
                .flatten()
                .sorted()
                .collect_vec();

            assert_eq!(children, ["child.1", "child.2", "child.3"]);
        }

        // Test with a filter
        {
            let mut walker = TreeWalker::new(child_of, name()).filter(a().with());

            let mut borrow = walker.borrow(&world);

            let mut root = borrow.root(root).unwrap();

            assert_eq!(root.get(), Some(&"root".into()));

            let children = root.map_children(|v| v.cloned()).sorted().collect_vec();

            assert_eq!(
                children,
                [None, Some("child.1".into()), Some("child.3".into())]
            );

            let mut paths: Vec<(Vec<Option<String>>, usize)> = Vec::new();

            fn traverse(
                mut node: TreeCursor<Component<String>, And<All, With>>,
                path: &[Option<String>],
                paths: &mut Vec<(Vec<Option<String>>, usize)>,
                depth: usize,
            ) {
                let name = node.get().cloned();
                eprintln!("{depth}: {name:?}");
                let path = path.iter().cloned().chain([name]).collect_vec();
                paths.push((path.clone(), depth));

                let mut children = node.children();

                while let Some(node) = children.next_child() {
                    traverse(node, &path, paths, depth + 1)
                }
            }

            traverse(root, &[], &mut paths, 0);

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
    }
}
