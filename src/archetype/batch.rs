use core::mem;

use alloc::{collections::BTreeMap, vec::Vec};

use crate::{error::Result, Component, ComponentInfo, ComponentKey, ComponentValue, Entity, Error};

use super::Storage;

/// Allows batch spawning many entities with the same components
#[derive(Debug)]
pub struct BatchSpawn {
    len: usize,
    storage: BTreeMap<ComponentKey, Storage>,
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
    pub fn components(&self) -> impl Iterator<Item = ComponentInfo> + '_ {
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
    ) -> Result<&mut Self> {
        let info = component.info();
        let mut storage = Storage::with_capacity(info, self.len);

        for item in iter.into_iter().take(self.len) {
            // Type gurangeed by the component
            unsafe { storage.push(item) }
        }

        debug_assert_eq!(storage.capacity(), self.len());

        self.append(storage)?;
        Ok(self)
    }

    /// Inserts a storage directly
    pub(crate) fn append(&mut self, storage: Storage) -> Result<()> {
        let info = storage.info();
        if storage.len() != self.len {
            Err(Error::IncompleteBatch)
        } else {
            self.storage.insert(info.key(), storage);
            Ok(())
        }
    }

    pub(crate) fn take_all(&mut self) -> impl Iterator<Item = (ComponentKey, Storage)> {
        mem::take(&mut self.storage).into_iter()
    }

    /// Spawns the batch into the world
    pub fn spawn(&mut self, world: &mut crate::World) -> Vec<Entity> {
        world.spawn_batch(self)
    }

    /// Spawns the batch into the world at the specified ids.
    pub fn spawn_at<'a>(
        &mut self,
        world: &mut crate::World,
        ids: &'a [Entity],
    ) -> Result<&'a [Entity]> {
        world.spawn_batch_at(ids, self)
    }
}

impl From<&mut BatchSpawn> for BatchSpawn {
    fn from(v: &mut BatchSpawn) -> Self {
        let len = v.len();
        mem::replace(v, BatchSpawn::new(len))
    }
}

#[cfg(test)]
mod test {

    use core::iter::repeat;

    use glam::{Mat4, Vec3};
    use itertools::Itertools;

    use crate::{component, components::name, FetchExt, Query, World};

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
        let mut batch = BatchSpawn::new(100);

        batch
            .set(transform(), repeat(Mat4::from_scale(Vec3::ONE)))
            .unwrap();

        batch
            .set(position(), (0..).map(|i| Vec3::X * i as f32))
            .unwrap();
        batch.set(rotation(), repeat(Vec3::X)).unwrap();
        batch.set(velocity(), repeat(Vec3::X)).unwrap();
        batch.spawn(&mut world);

        pretty_assertions::assert_eq!(
            Query::new((
                transform().copied(),
                position().copied(),
                rotation().copied(),
                velocity().copied()
            ))
            .borrow(&world)
            .iter()
            .collect_vec(),
            (0..100)
                .map(|i| (
                    Mat4::from_scale(Vec3::ONE),
                    Vec3::X * i as f32,
                    Vec3::X,
                    Vec3::X
                ))
                .collect_vec()
        );
    }
}
