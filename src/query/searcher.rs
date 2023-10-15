use alloc::vec::Vec;
use core::cmp;

use crate::{
    archetype::{Archetype, ArchetypeId},
    archetypes::Archetypes,
    component::ComponentKey,
};

#[derive(Default, Debug, Clone)]
/// Declares search terms for a queries archetypes
pub struct ArchetypeSearcher {
    pub(crate) required: Vec<ComponentKey>,
}

impl ArchetypeSearcher {
    /// Add a required component
    pub fn add_required(&mut self, component: ComponentKey) {
        self.required.push(component)
    }

    #[inline]
    pub(crate) fn find_archetypes<'a>(
        &mut self,
        archetypes: &'a Archetypes,
        mut result: impl FnMut(ArchetypeId, &'a Archetype),
    ) {
        self.required.sort();
        self.required.dedup();

        traverse_archetypes(archetypes, archetypes.root(), &self.required, &mut result);
    }
}

#[inline]
pub(crate) fn traverse_archetypes<'a>(
    archetypes: &'a Archetypes,
    cur: ArchetypeId,
    required: &[ComponentKey],
    result: &mut impl FnMut(ArchetypeId, &'a Archetype),
) {
    let arch = archetypes.get(cur);
    match required {
        // All components are found, every archetype from now on is matched
        [] => {
            // This matches
            result(cur, arch);

            for &arch_id in arch.children.values() {
                traverse_archetypes(archetypes, arch_id, required, result);
            }
        }
        [head, tail @ ..] => {
            // Since the components in the trie are in order, a value greater than head means the
            // current component will never occur
            for (&component, &arch_id) in &arch.children {
                match component.cmp(head) {
                    cmp::Ordering::Less => {
                        // Not quite, keep looking
                        traverse_archetypes(archetypes, arch_id, required, result);
                    }
                    cmp::Ordering::Equal => {
                        // One more component has been found, continue to search for the remaining ones
                        traverse_archetypes(archetypes, arch_id, tail, result);
                    }
                    cmp::Ordering::Greater => {
                        // We won't find anything of interest further down the tree
                    }
                }
            }
        }
    }
}
