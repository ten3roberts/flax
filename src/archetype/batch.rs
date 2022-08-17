use std::{collections::BTreeMap, mem};

use crate::{Component, ComponentId, ComponentInfo, ComponentValue, Entity, Error};

use super::Storage;

pub struct BatchSpawn {
    len: usize,
    storage: BTreeMap<ComponentId, Storage>,
}

impl BatchSpawn {
    pub fn new(len: usize) -> Self {
        Self {
            len,
            storage: Default::default(),
        }
    }

    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.storage.values().map(|v| v.info())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Set values for a specific component. The number of the items in the iterator
    /// must match the `len` given to the `BatchSpawn`
    pub fn set<T: ComponentValue>(
        &mut self,
        component: Component<T>,
        iter: impl IntoIterator<Item = T>,
    ) -> crate::error::Result<()> {
        let info = component.info();
        let mut storage = Storage::with_capacity(info, self.len);

        for item in iter.into_iter().take(self.len) {
            storage.push(item)
        }

        self.insert(storage)
    }

    /// Inserts a storage directly
    pub(crate) fn insert(&mut self, storage: Storage) -> crate::error::Result<()> {
        let info = storage.info();
        if storage.len() != self.len {
            Err(Error::IncompleteBatch)
        } else {
            self.storage.insert(info.id(), storage);
            Ok(())
        }
    }

    pub(crate) fn take_all(&mut self) -> impl Iterator<Item = (ComponentId, Storage)> {
        mem::take(&mut self.storage).into_iter()
    }

    pub fn spawn(&mut self, world: &mut crate::World) -> Vec<Entity> {
        world.spawn_batch(self)
    }
}

#[cfg(test)]
mod test {
    use crate::{component, components::name, World};

    use super::*;
    #[test]
    fn component_batch() {
        component! {
            pos: (f32, f32),
        }

        let mut batch = BatchSpawn::new(8);

        batch
            .set(
                pos(),
                [
                    (1.0, 3.0),
                    (5.0, 2.9),
                    (6.7, 9.3),
                    (7.0, 3.4),
                    (6.7, 9.3),
                    (5.6, 1.3),
                    (4.7, 8.1),
                    (5.3, 3.5),
                ],
            )
            .unwrap();

        batch.set(name(), ('a'..).map(|v| v.to_string())).unwrap();

        let mut world = World::new();
        let ids = world.spawn_batch(&mut batch);

        for (&id, n) in ids.iter().zip(('a'..).map(|v| v.to_string())) {
            assert_eq!(world.get(id, name()).as_deref(), Ok(&n));
        }
    }
}
