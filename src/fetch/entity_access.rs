use core::ops::Deref;

use atomic_refcell::AtomicRef;

use crate::{Component, ComponentValue, Entity, EntityIds, EntityRef, Mutable, RefMut};

use super::{copied::Copied, Cloned};

/// Generically access an entity in the world
pub trait EntityAccess<'a> {
    /// The produced item
    type Item;
    /// Access the entity
    fn get(&'a self, entity: &'a EntityRef) -> Option<Self::Item>;
}

impl<'a, T: ComponentValue> EntityAccess<'a> for Component<T> {
    type Item = AtomicRef<'a, T>;

    fn get(&self, entity: &'a EntityRef) -> Option<Self::Item> {
        entity.get(*self).ok()
    }
}

impl<'a, T: ComponentValue> EntityAccess<'a> for Mutable<T> {
    type Item = RefMut<'a, T>;

    fn get(&self, entity: &'a EntityRef) -> Option<Self::Item> {
        entity.get_mut(self.0).ok()
    }
}

impl<'a> EntityAccess<'a> for EntityIds {
    type Item = Entity;

    fn get(&'a self, entity: &'a EntityRef) -> Option<Self::Item> {
        Some(entity.id())
    }
}

impl<'a, T: EntityAccess<'a>, V> EntityAccess<'a> for Cloned<T>
where
    T::Item: Deref<Target = V>,
    V: 'a + Clone,
{
    type Item = V;

    fn get(&'a self, entity: &'a EntityRef) -> Option<Self::Item> {
        Some(self.0.get(entity)?.clone())
    }
}

impl<'a, T: EntityAccess<'a>, V> EntityAccess<'a> for Copied<T>
where
    T::Item: Deref<Target = V>,
    V: 'a + Copy,
{
    type Item = V;

    fn get(&'a self, entity: &'a EntityRef) -> Option<Self::Item> {
        Some(*self.0.get(entity)?)
    }
}
