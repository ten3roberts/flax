mod borrow;
mod dfs;

use alloc::collections::{btree_map, BTreeMap, BTreeSet};
use smallvec::SmallVec;

use crate::filter::Filtered;
use crate::{
    archetype::Archetype, component_info, entity::EntityLocation, All, ArchetypeId,
    ArchetypeSearcher, Archetypes, ComponentKey, ComponentValue, Entity, Fetch, RelationExt, World,
};

use self::borrow::QueryBorrowState;
use self::dfs::*;

/// Provides utilities for working with and manipulating hierarchies and graphs

/// Allows traversing an entity graph following `relation`
pub struct GraphQuery<Q, F> {
    fetch: Filtered<Q, F>,

    change_tick: u32,
    archetype_gen: u32,
    include_components: bool,

    strategy: Dfs,
    state: Option<DfsState>,
}

impl<Q> GraphQuery<Q, All> {
    pub fn with_strategy(fetch: Q, strategy: Dfs) -> Self {
        Self {
            fetch: Filtered::new(fetch, All),
            change_tick: 0,
            archetype_gen: 0,
            include_components: false,
            strategy,
            state: None,
        }
    }
}

type AdjMap<'a> = BTreeMap<Entity, SmallVec<[(ArchetypeId, &'a Archetype); 8]>>;

impl<Q, F> GraphQuery<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    /// Prepare the next change tick and return the old one for the last time
    /// the query ran
    fn prepare_tick(&mut self, world: &World) -> (u32, u32) {
        // The tick of the last iteration
        let mut old_tick = self.change_tick;

        // Set the change_tick for self to that of the query, to make all
        // changes before this invocation too old
        //
        // It is only necessary to acquire a new change tick if the query will
        // change anything

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
        (old_tick, new_tick)
    }

    pub fn borrow<'w>(&'w mut self, world: &'w World) -> DfsBorrow<'w, Q, F> {
        let (old_tick, new_tick) = self.prepare_tick(world);
        // Make sure the archetypes to visit are up to date
        let archetype_gen = world.archetype_gen();
        if archetype_gen > self.archetype_gen {
            self.state = None;
            self.archetype_gen = archetype_gen;
        }

        let state = self.state.get_or_insert_with(|| {
            let mut searcher = ArchetypeSearcher::default();
            self.fetch.searcher(&mut searcher);
            if !self.include_components {
                searcher.add_excluded(component_info().key());
            }

            let filter = |arch: &Archetype| self.fetch.filter_arch(arch);

            self.strategy.get_state(world, searcher, filter)
        });

        let query_state = QueryBorrowState { old_tick, new_tick };

        DfsBorrow::new(world, &self.fetch, query_state, state)
    }
}

#[derive(Debug, Clone)]
struct DfsState {
    archetypes: Vec<ArchetypeId>,
    archetypes_index: BTreeMap<ArchetypeId, usize>,
    adj: BTreeMap<Entity, Vec<usize>>,
    root: Entity,
}

impl DfsState {
    pub fn insert_arch(&mut self, arch_id: ArchetypeId) -> usize {
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

pub struct Dfs {
    root: Entity,
    relation: Entity,
}

impl Dfs {
    pub fn new<T: ComponentValue>(relation: impl RelationExt<T>, root: Entity) -> Self {
        Self {
            relation: relation.id(),
            root,
        }
    }

    fn get_state<F: Fn(&Archetype) -> bool>(
        &self,
        world: &World,
        searcher: ArchetypeSearcher,
        filter: F,
    ) -> DfsState {
        let archetypes = &world.archetypes;

        struct SearchState<'a, F> {
            archetypes: &'a Archetypes,
            searcher: &'a ArchetypeSearcher,
            filter: &'a F,
            relation: Entity,
            result: DfsState,
            visited: BTreeSet<ArchetypeId>,
        }

        fn inner<F: Fn(&Archetype) -> bool>(
            state: &mut SearchState<F>,
            loc: EntityLocation,
            _: &Archetype,
            _: usize,
            id: Entity,
        ) {
            eprintln!("Visiting {id} {loc:?}");

            dbg!(&state.visited);

            // Find all archetypes for the children of parent
            let key = ComponentKey::new(state.relation, Some(id));

            let mut searcher = state.searcher.clone();
            searcher.add_required(key);

            let mut children = Vec::new();
            searcher.find_archetypes(state.archetypes, |arch_id, arch| {
                if !(state.filter)(arch) {
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
            filter: &filter,
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

        dbg!(&state.result);
        state.result
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{child_of, entity_ids, name, CommandBuffer, FetchExt, Query};

    use super::*;

    #[test]
    fn traverse_dfs() {
        let mut world = World::new();

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
            GraphQuery::with_strategy((name().cloned(), a().copied()), Dfs::new(child_of, root));

        let items = query.borrow(&world).iter().collect_vec();

        assert_eq!(
            items,
            [
                ("root".to_string(), 0),
                ("child.2".to_string(), 3),
                ("child.1".to_string(), 1),
                ("child.1.1".to_string(), 2),
                ("child.3".to_string(), 4)
            ]
        );

        let mut cmd = CommandBuffer::new();

        GraphQuery::with_strategy((entity_ids(), name()), Dfs::new(child_of, root))
            .borrow(&world)
            .cascade(&Vec::new(), |(id, name), prefix| {
                let mut p = prefix.clone();
                p.push(name.clone());

                cmd.set(id, path(), p.join("::"));
                p
            });

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

        let mut query = GraphQuery::with_strategy(
            (name().cloned(), a().modified().copied()),
            Dfs::new(child_of, root),
        );

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
            .for_each(|(id, a)| {
                eprintln!("Writing to: {id}");
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
