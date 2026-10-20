use alloc::vec::Vec;

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
        mut result: impl FnMut(ArchetypeId, &'a Archetype) -> bool,
    ) {
        self.required.sort();
        self.required.dedup();

        match &self.required[..] {
            [] => {
                for (id, arch) in archetypes.iter() {
                    if result(id, arch) {
                        return;
                    }
                }
            }
            [head, tail @ ..] => {
                for (&id, _) in archetypes.index.find(*head).into_iter().flatten() {
                    // for (id, _) in archetypes.iter() {
                    let arch = archetypes.get(id);

                    if arch.has_all(tail) {
                        if result(id, arch) {
                            return;
                        }
                    }
                }
            }
        }
    }
}
