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
pub struct ComponentBuffer {
    inserts: BufferStorage,
    insert_locations: BTreeMap<(Entity, ComponentInfo), usize>,
    removals: Vec<(Entity, ComponentId)>,
}

impl ComponentBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Deferred set a component for `id`.
    /// Unlike, [`World::insert`] it does not return the old value as that is
    /// not known at call time.
    pub fn set<T: ComponentValue>(
        &mut self,
        id: Entity,
        component: Component<T>,
        value: T,
    ) -> &mut Self {
        match self.insert_locations.entry((id, component.info())) {
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
    pub fn remove<T: ComponentValue>(&mut self, id: Entity, component: Component<T>) -> &mut Self {
        let offset = self.insert_locations.remove(&(id, component.info()));

        // Remove from insert list
        if let Some(offset) = offset {
            unsafe { self.inserts.take::<T>(offset) };
            eprintln!("Found old value");
        }

        self.removals.push((id, component.id()));

        self
    }

    /// Applies all contents of the command buffer to the world.
    /// The commandbuffer is cleared and can be reused.
    pub fn apply(&mut self, world: &mut World) -> Result<(), Error> {
        let groups = self
            .insert_locations
            .iter()
            .group_by(|((entity, _), _)| *entity);

        let storage = &mut self.inserts;
        let result = (&groups).into_iter().map(|(id, group)| {
            // Safety
            // The offset is acquired from the map which was previously acquired
            unsafe {
                let components =
                    group.map(|((_, info), offset)| (*info, storage.take_dyn(*offset)));
                world.set_with(id, components)
            }
        });

        todo!()
    }

    fn entity_inserts(&self, id: Entity) -> impl Iterator<Item = usize> + '_ {
        self.insert_locations
            .iter()
            .skip_while(move |((entity, _), _)| *entity < id)
            .take_while(move |((entity, _), _)| *entity == id)
            .map(|(_, offset)| *offset)
    }
}

struct TupleIterator<'a> {
    map_iter: Peekable<btree_map::Iter<'a, (Entity, ComponentId), usize>>,
}

impl<'a> TupleIterator<'a> {
    fn next_chunk(&'a mut self) -> Option<LocationIter> {
        let ((id, _), _) = self.map_iter.peek()?;
        Some(LocationIter {
            iter: &mut self.map_iter,
            id: *id,
        })
    }
}

struct LocationIter<'a> {
    iter: &'a mut Peekable<btree_map::Iter<'a, (Entity, ComponentId), usize>>,
    id: Entity,
}

impl<'a> Iterator for LocationIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next_if(|((id, _), _)| *id == self.id)
            .map(|(_, offset)| *offset)
    }
}
