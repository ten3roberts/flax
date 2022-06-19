use std::{
    collections::{
        btree_map::{self, Entry},
        BTreeMap,
    },
    iter::Peekable,
    process::id,
};

use itertools::Itertools;

use crate::{
    BufferStorage, Component, ComponentId, ComponentInfo, ComponentValue, Entity, Error, World,
};

/// Records commands into the world.
/// Allows insertion and removal of components when the world is not available
/// mutably, such as in systems or during iteration.
#[derive(Default, Debug)]
pub struct CommandBuffer {
    inserts: BufferStorage,
    insert_locations: BTreeMap<(Entity, ComponentInfo), usize>,
    removals: Vec<(Entity, ComponentInfo)>,
}

impl CommandBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Deferred set a component for `id`.
    /// Unlike, [`World::insert`] it does not return the old value as that is
    /// not known at call time.
    pub fn set<T: ComponentValue>(
        &mut self,
        id: impl Into<Entity>,
        component: Component<T>,
        value: T,
    ) -> &mut Self {
        match self.insert_locations.entry((id.into(), component.info())) {
            Entry::Vacant(slot) => {
                let offset = self.inserts.insert(value);
                slot.insert(offset);
            }
            Entry::Occupied(slot) => unsafe {
                self.inserts.swap(*slot.get(), value);
            },
        }

        self
    }

    /// Deferred removal of a component for `id`.
    /// Unlike, [`World::remove`] it does not return the old value as that is
    /// not known at call time.
    pub fn remove<T: ComponentValue>(
        &mut self,
        id: impl Into<Entity>,
        component: Component<T>,
    ) -> &mut Self {
        let id = id.into();
        let offset = self.insert_locations.remove(&(id, component.info()));

        // Remove from insert list
        if let Some(offset) = offset {
            unsafe { self.inserts.take::<T>(offset) };
            eprintln!("Found old value");
        }

        self.removals.push((id, component.info()));

        self
    }

    /// Applies all contents of the command buffer to the world.
    /// The commandbuffer is cleared and can be reused.
    ///
    /// Returns a vec of all errors encountered.
    /// If an error is encountered the commandbuffer will add it to the list and
    /// continue with the rest of the items.
    pub fn apply(&mut self, world: &mut World) -> Result<(), Vec<Error>> {
        let groups = self
            .insert_locations
            .iter()
            .group_by(|((entity, _), _)| *entity);

        let storage = &mut self.inserts;
        let mut errors = Vec::new();
        let inserted = (&groups)
            .into_iter()
            .map(|(id, group)| {
                // Safety
                // The offset is acquired from the map which was previously acquired
                unsafe {
                    let components =
                        group.map(|((_, info), offset)| (*info, storage.take_dyn(*offset)));
                    world.set_with(id, components)
                }
            })
            .flat_map(Result::err);

        errors.extend(inserted);

        let removed = self
            .removals
            .drain(..)
            .map(|(id, component)| world.remove_dyn(id, component))
            .flat_map(Result::err);

        errors.extend(removed);

        self.clear();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn clear(&mut self) {
        self.inserts.clear();
        self.insert_locations.clear();
        self.removals.clear();
    }
}
