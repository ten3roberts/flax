use alloc::collections::{btree_map, BTreeMap, BTreeSet};

use crate::{
    archetype::Archetype, component_info, visitors, ArchetypeId, ArchetypeSearcher, Archetypes,
    Component, ComponentKey, Entity, Fetch, World,
};

#[derive(Debug, Clone)]
/// A query which allows visiting
pub struct RecursiveQuery<Q, F> {
    fetch: Q,
    filter: F,

    archetypes: Vec<ArchetypeId>,
    change_tick: u32,
    archetype_gen: u32,
    include_components: bool,

    relation: Entity,
}

impl<Q, F> RecursiveQuery<Q, F>
where
    Q: for<'x> Fetch<'x>,
    F: for<'x> Fetch<'x>,
{
    pub fn get_archetypes(&self, world: &World) {
        let mut searcher = ArchetypeSearcher::default();
        self.fetch.searcher(&mut searcher);
        if !self.include_components {
            searcher.add_excluded(component_info().key());
        }

        let archetypes = &world.archetypes;

        let filter = |arch: &Archetype| {
            self.fetch.matches(arch)
                && self.filter.matches(arch)
                && (!Q::HAS_FILTER || self.fetch.filter().matches(arch))
        };

        let mut result = BTreeMap::new();
        searcher.find_archetypes(archetypes, |arch_id, arch| {
            if filter(arch) {
                let arch = result.insert(arch_id, arch);
                assert!(arch.is_none(), "Archetype found twice");
            }
        });

        // Perform a bottoms up topological search
        let mut visited = Default::default();
        let mut ordered = Default::default();

        for (&arch_id, &arch) in &result {
            get_ordered_archetypes(
                world,
                &result,
                arch_id,
                arch,
                self.relation,
                &mut visited,
                &mut ordered,
            );
        }
    }
}

enum VisitedState {
    Pending,
    Visited(bool),
}

fn get_ordered_archetypes(
    world: &World,
    archetypes: &BTreeMap<ArchetypeId, &Archetype>,
    arch_id: ArchetypeId,
    arch: &Archetype,
    relation: Entity,
    visited: &mut BTreeMap<ArchetypeId, VisitedState>,
    ordered: &mut Vec<ArchetypeId>,
) -> bool {
    match visited.entry(arch_id) {
        btree_map::Entry::Vacant(entry) => {
            entry.insert(VisitedState::Pending);
        }
        btree_map::Entry::Occupied(entry) => match entry.get() {
            VisitedState::Pending => panic!("Cyclic"),
            &VisitedState::Visited(is_reachable) => return is_reachable,
        },
    }

    // Find relations to other objects, and visit them as well
    let relations = arch.relations_like(relation);
    let mut is_reachable = false;
    let mut is_root = true;
    for (key, _) in relations {
        is_root = false;
        let parent = key.object.unwrap();

        let loc = world.location(parent).unwrap();
        // Part of the visited set
        if let Some(arch) = archetypes.get(&loc.arch_id) {
            if get_ordered_archetypes(world, archetypes, arch_id, arch, relation, visited, ordered)
            {
                is_reachable = true;
            }
        }
    }

    let is_reachable = is_reachable || is_root;

    visited.insert(arch_id, VisitedState::Visited(is_reachable));

    ordered.push(arch_id);
    is_reachable
}

// fn recurse_entity(
//     filter: &impl Fn(ArchetypeId, &Archetype) -> bool,
//     world: &World,
//     archetypes: &Archetypes,
//     relation: Entity,
//     parent: Entity,
//     base_searcher: &ArchetypeSearcher,
//     result: &mut Vec<ArchetypeId>,
// ) -> Vec<ArchetypeId> {
//     let mut searcher = base_searcher.clone();

//     searcher.add_required(ComponentKey::new(relation, Some(parent)));

//     searcher.find_archetypes_with(archetypes, |arch_id, arch| {
//         if filter(arch_id, arch) {
//             eprintln!("Found archetypes: {arch_id}");
//             // Look at each entity in the archetype, and find
//         }
//     });
// }
