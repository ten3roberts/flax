use alloc::collections::{btree_map, BTreeMap, BTreeSet};
use itertools::sorted;

use crate::{ArchetypeId, ComponentValue, Entity, Fetch, RelationExt};

use super::ArchetypeSearcher;

pub struct Topo {
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
        let mut adj: BTreeMap<Entity, Vec<usize>> = BTreeMap::new();
        let mut deps: BTreeMap<usize, _> = BTreeMap::new();

        searcher.find_archetypes(&world.archetypes, |arch_id, arch| {
            if !fetch.filter_arch(arch) {
                return;
            }

            let arch_index = match self.archetypes_index.entry(arch_id) {
                btree_map::Entry::Vacant(slot) => {
                    let idx = self.archetypes.len();
                    self.archetypes.push(arch_id);
                    *slot.insert(idx)
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
                deps.insert(arch_index, arch_deps);
            }
        });

        enum VisitedState {
            Pending,
            Visited,
        }

        fn sort(
            order: &mut Vec<usize>,
            visited: &mut BTreeSet<usize>,
            index: &BTreeMap<ArchetypeId, usize>,
            deps: &BTreeMap<usize, Vec<ArchetypeId>>,
            arch_id: ArchetypeId,
            arch_index: usize,
        ) {
            if !visited.insert(arch_index) {
                eprintln!("Archetype {arch_index} {arch_id} already visited");
                return;
            }

            // Make sure all dependencies i.e; parents, are visited first
            for dep in deps.get(&arch_index).into_iter().flatten() {
                if let Some(&arch_index) = index.get(dep) {
                    sort(order, visited, index, deps, arch_id, arch_index);
                } else {
                    eprintln!("Parent is not part of the fetch")
                }
            }

            eprintln!("=> {arch_index} {arch_id}");
            order.push(arch_index);
        }
        eprintln!("Sorting topo");

        let mut visited = BTreeSet::new();
        for (arch_index, &arch_id) in self.archetypes.iter().enumerate() {
            dbg!(arch_index, arch_id);
            sort(
                &mut self.order,
                &mut visited,
                &self.archetypes_index,
                &deps,
                arch_id,
                arch_index,
            )
        }
    }

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
    }
}

impl Topo {
    pub fn new<T: ComponentValue>(relation: impl RelationExt<T>) -> Self {
        Self {
            relation: relation.id(),
        }
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use crate::{child_of, component_info, name, World};
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

                eprintln!(
                    "{:?}",
                    arch.borrow::<String>(name().key()).unwrap().to_vec()
                );
                arch.entities().to_vec()
            })
            .collect_vec();

        assert_eq!(
            visited,
            [vec![a, d], vec![g], vec![f], vec![c], vec![], vec![e, b]]
        );
    }
}
