use alloc::vec::Vec;
use core::cmp;
use itertools::Itertools;

use crate::{archetype::Archetype, ArchetypeId, Archetypes, ComponentKey, Entity};

#[derive(Default, Debug, Clone)]
/// Declares search terms for a queries archetypes
pub struct ArchetypeSearcher {
    pub(crate) required: Vec<ComponentKey>,
    excluded: Vec<ComponentKey>,
    excluded_relations: Vec<Entity>,
}

impl ArchetypeSearcher {
    /// Add a required component
    pub fn add_required(&mut self, component: ComponentKey) {
        self.required.push(component)
    }

    /// Add an excluded component
    pub fn add_excluded(&mut self, component: ComponentKey) {
        self.excluded.push(component)
    }

    /// Add an excluded relation type
    pub fn add_excluded_relation(&mut self, relation: Entity) {
        self.excluded_relations.push(relation)
    }

    #[inline]
    pub(crate) fn find_archetypes<'a>(
        &mut self,
        archetypes: &'a Archetypes,
        mut result: impl FnMut(ArchetypeId, &'a Archetype),
    ) {
        dbg!(&self);
        self.required.sort();
        self.required.dedup();

        traverse_archetypes(
            archetypes,
            archetypes.root(),
            &self.required,
            &self.excluded,
            &mut result,
        );
    }
}

#[inline]
pub(crate) fn traverse_archetypes<'a>(
    archetypes: &'a Archetypes,
    cur: ArchetypeId,
    required: &[ComponentKey],
    excluded: &[ComponentKey],
    result: &mut impl FnMut(ArchetypeId, &'a Archetype),
) {
    let arch = archetypes.get(cur);
    dbg!(cur, required, arch.component_names().collect_vec());
    match required {
        // All components are found, every archetype from now on is matched
        [] => {
            // This matches
            eprintln!(
                "Found archetype: {:?}",
                arch.component_names().collect_vec()
            );
            result(cur, arch);

            dbg!(&arch.children);
            for (&component, &arch_id) in &arch.children {
                // Oops, don't even step on it
                if excluded.contains(&component) {
                    continue;
                }
                traverse_archetypes(archetypes, arch_id, required, excluded, result);
            }
        }
        [head, tail @ ..] => {
            // Since the components in the trie are in order, a value greater than head means the
            // current component will never occur
            for (&component, &arch_id) in &arch.outgoing {
                // Oops, don't even step on it
                if excluded.contains(&component) {
                    continue;
                }

                match component.cmp(head) {
                    cmp::Ordering::Less => {
                        // Not quite, keep looking
                        traverse_archetypes(archetypes, arch_id, required, excluded, result);
                    }
                    cmp::Ordering::Equal => {
                        // One more component has been found, continue to search for the remaining ones
                        traverse_archetypes(archetypes, arch_id, tail, excluded, result);
                    }
                    cmp::Ordering::Greater => {
                        // We won't find anything of interest further down the tree
                    }
                }
            }
        }
    }
}