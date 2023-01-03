use alloc::vec::Vec;
use core::cmp;

use crate::{archetype::Archetype, ArchetypeId, Archetypes, ComponentKey};

#[derive(Default, Debug, Clone)]
/// Declares search terms for a queries archetypes
pub struct ArchetypeSearcher {
    pub(crate) required: Vec<ComponentKey>,
    excluded: Vec<ComponentKey>,
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

    pub(crate) fn find_archetypes(
        &mut self,
        archetypes: &Archetypes,
        filter: impl Fn(ArchetypeId, &Archetype) -> bool,
    ) -> Vec<ArchetypeId> {
        self.required.sort();
        self.required.dedup();

        let mut result = Vec::new();
        traverse_archetypes(
            archetypes,
            archetypes.root(),
            &self.required,
            &self.excluded,
            &mut result,
            &filter,
        );

        result
    }
}

#[inline]
fn traverse_archetypes<F: Fn(ArchetypeId, &Archetype) -> bool>(
    archetypes: &Archetypes,
    cur: ArchetypeId,
    components: &[ComponentKey],
    excluded: &[ComponentKey],
    result: &mut Vec<ArchetypeId>,
    filter: &F,
) {
    let arch = archetypes.get(cur);
    match components {
        // All components are found, every archetype from now on is matched
        [] => {
            // This matches
            if filter(cur, arch) {
                result.push(cur);
            }

            for (&component, &arch_id) in &arch.children {
                // Oops, don't even step on it
                if excluded.contains(&component) {
                    continue;
                }
                traverse_archetypes(archetypes, arch_id, components, excluded, result, filter);
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
                        traverse_archetypes(
                            archetypes, arch_id, components, excluded, result, filter,
                        );
                    }
                    cmp::Ordering::Equal => {
                        // One more component has been found, continue to search for the remaining ones
                        traverse_archetypes(archetypes, arch_id, tail, excluded, result, filter);
                    }
                    cmp::Ordering::Greater => {
                        // We won't find anything of interest further down the tree
                    }
                }
            }
        }
    }
}
