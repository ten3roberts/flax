use core::mem;

use alloc::{collections::BTreeMap, vec::Vec};

use crate::{Component, ComponentId, ComponentInfo, ComponentValue, Entity, Error};

use super::Storage;

/// Allows batch spawning many entities with the same components
#[derive(Debug)]
pub struct BatchSpawn {
    len: usize,
    storage: BTreeMap<ComponentId, Storage>,
}

impl BatchSpawn {
    /// Creates a new batch spawn to spawn `len` entities
    pub fn new(len: usize) -> Self {
        Self {
            len,
            storage: Default::default(),
        }
    }

    /// Returns the components in the batch
    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.storage.values().map(|v| v.info())
    }

    /// Returns the number of entities in the batch
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    /// Returns true if the batch will not spawn any entities
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Set values for a specific component. The number of the items in the iterator
    /// must match the `len` given to the `BatchSpawn`
    pub fn set<T: ComponentValue>(
        &mut self,
        component: Component<T>,
        iter: impl IntoIterator<Item = T>,
    ) -> crate::error::Result<&mut Self> {
        let info = component.info();
        let mut storage = Storage::with_capacity(info, self.len);

        for item in iter.into_iter().take(self.len) {
            storage.push(item)
        }

        debug_assert_eq!(storage.capacity(), self.len());

        self.append(storage)?;
        Ok(self)
    }

    /// Inserts a storage directly
    pub(crate) fn append(&mut self, storage: Storage) -> crate::error::Result<()> {
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

    /// Spawns the batch into the world
    pub fn spawn(&mut self, world: &mut crate::World) -> Vec<Entity> {
        world.spawn_batch(self)
    }
}

#[cfg(test)]
mod test {

    use core::iter::repeat;

    use glam::{Mat4, Vec3};

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

        batch.set(name(), ('a'..).map(|v| v.into())).unwrap();

        let mut world = World::new();
        let ids = world.spawn_batch(&mut batch);

        for (&id, n) in ids.iter().zip(('a'..).map(|v| v.into())) {
            assert_eq!(world.get(id, name()).as_deref(), Ok(&n));
        }
    }

    #[test]
    fn batch_spawn() {
        component! {
            transform: Mat4,
            position: Vec3,
            rotation: Vec3,
            velocity: Vec3,
        }

        let mut world = World::new();
        let mut batch = BatchSpawn::new(10_000);

        batch
            .set(transform(), repeat(Mat4::from_scale(Vec3::ONE)))
            .unwrap();

        batch.set(position(), repeat(Vec3::X)).unwrap();
        batch.set(rotation(), repeat(Vec3::X)).unwrap();
        batch.set(velocity(), repeat(Vec3::X)).unwrap();
        batch.spawn(&mut world);
    }
}
