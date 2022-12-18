use core::fmt::Debug;
use core::mem::MaybeUninit;

use atomic_refcell::{AtomicRef, AtomicRefMut};

use crate::{
    entity::EntityLocation,
    entry::{Entry, OccupiedEntry, VacantEntry},
    error::Result,
    Component, ComponentKey, ComponentValue, Entity, EntityFormatter, Error, World,
};

/// Borrow all the components of an entity at once.
///
/// This is handy to borrow an entity and perform multiple operations on it
/// without mentioning the id and performing re-lookups.
pub struct EntityRefMut<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) loc: EntityLocation,
    pub(crate) id: Entity,
}

impl<'a> EntityRefMut<'a> {
    /// Access a component
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Result<AtomicRef<T>> {
        self.world
            .get_at(self.loc, component)
            .ok_or_else(|| Error::MissingComponent(self.id, component.info()))
    }

    /// Access a component mutably
    pub fn get_mut<T: ComponentValue>(&self, component: Component<T>) -> Result<AtomicRefMut<T>> {
        self.world
            .get_mut_at(self.loc, component)
            .ok_or_else(|| Error::MissingComponent(self.id, component.info()))
    }

    /// Check if the entity currently has the specified component without
    /// borrowing.
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.world
            .archetypes
            .get(self.loc.arch_id)
            .has(component.key())
    }

    /// Set a component for the entity
    pub fn set<T: ComponentValue>(
        &mut self,
        component: Component<T>,
        value: T,
    ) -> Result<Option<T>> {
        let (old, loc) = self.world.set_inner(self.id, component, value)?;
        self.loc = loc;
        Ok(old)
    }

    /// Remove a component
    pub fn remove<T: ComponentValue>(&mut self, component: Component<T>) -> Result<T> {
        let mut res: MaybeUninit<T> = MaybeUninit::uninit();
        let (old, loc) = unsafe {
            let loc = self.world.remove_inner(self.id, component.info(), |ptr| {
                res.write(ptr.cast::<T>().read());
            })?;
            (res.assume_init(), loc)
        };

        self.loc = loc;
        Ok(old)
    }

    /// Retain only the components specified by the predicate
    pub fn retain(&mut self, f: impl FnMut(ComponentKey) -> bool) {
        self.loc = self.world.retain_entity_components(self.id, self.loc, f)
    }

    /// See: [`crate::World::clear`]
    pub fn clear(&mut self) {
        self.retain(|_| false)
    }

    /// Returns the entity id
    pub fn id(&self) -> Entity {
        self.id
    }

    /// See [`crate::World::entry`]
    pub fn entry<T: ComponentValue>(self, component: Component<T>) -> Entry<'a, T> {
        if self.has(component) {
            Entry::Occupied(OccupiedEntry {
                borrow: self.world.get_mut_at(self.loc, component).unwrap(),
            })
        } else {
            Entry::Vacant(VacantEntry {
                world: self.world,
                id: self.id,
                component,
            })
        }
    }

    /// Version of [`Self::downgrade`] which takes self by reference
    pub fn downgrade_ref(&mut self) -> EntityRef {
        EntityRef {
            world: self.world,
            loc: self.loc,
            id: self.id,
        }
    }

    /// Convert the [`EntityRefMut`] into a [`EntityRef`]
    pub fn downgrade(self) -> EntityRef<'a> {
        EntityRef {
            world: self.world,
            loc: self.loc,
            id: self.id,
        }
    }

    /// Returns a mutable reference to the contained world
    pub fn world_mut(&mut self) -> &mut World {
        self.world
    }

    /// Returns a reference to the contained world
    pub fn world(&self) -> &World {
        self.world
    }
}

/// Borrow all the components of an entity at once.
///
/// This is handy to borrow an entity and perform multiple operations on it
/// without mentioning the id and performing re-lookups.
#[derive(Copy, Clone)]
pub struct EntityRef<'a> {
    pub(crate) world: &'a World,
    pub(crate) loc: EntityLocation,
    pub(crate) id: Entity,
}

impl<'a> EntityRef<'a> {
    /// Access a component
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Result<AtomicRef<T>> {
        self.world
            .get_at(self.loc, component)
            .ok_or_else(|| Error::MissingComponent(self.id, component.info()))
    }

    /// Access a component mutably
    pub fn get_mut<T: ComponentValue>(&self, component: Component<T>) -> Result<AtomicRefMut<T>> {
        self.world
            .get_mut_at(self.loc, component)
            .ok_or_else(|| Error::MissingComponent(self.id, component.info()))
    }

    /// Check if the entity currently has the specified component without
    /// borrowing.
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.world
            .archetypes
            .get(self.loc.arch_id)
            .has(component.key())
    }

    /// Returns the entity id
    pub fn id(&self) -> Entity {
        self.id
    }
}

impl<'a> Debug for EntityRef<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        EntityFormatter {
            world: self.world,
            ids: &[self.id],
        }
        .fmt(f)
    }
}

impl<'a> Debug for EntityRefMut<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        EntityFormatter {
            world: self.world,
            ids: &[self.id],
        }
        .fmt(f)
    }
}

#[cfg(test)]
mod test {

    use crate::{component, components::name, is_static, EntityBuilder};

    use super::*;

    #[test]
    fn spawn_ref() {
        let mut world = World::new();

        let mut entity = world.spawn_ref();

        let id = entity.id();

        entity.set(name(), "Foo".into()).unwrap();

        assert_eq!(
            entity.world().get_mut(id, name()).as_deref(),
            Ok(&"Foo".into())
        );

        let entity = entity.downgrade();
        let res = entity.get(is_static());

        assert_eq!(
            res.as_deref(),
            Err(&Error::MissingComponent(id, is_static().info()))
        )
    }

    #[test]
    fn entity_ref() {
        component! {
            health: f32,
            pos: (f32, f32),
        }

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(name(), "Foo".into())
            .spawn(&mut world);

        let mut entity = world.entity_mut(id).unwrap();

        assert_eq!(entity.get(name()).as_deref(), Ok(&"Foo".into()));

        entity.set(health(), 100.0).unwrap();
        // panic!("");

        assert_eq!(entity.get(name()).as_deref(), Ok(&"Foo".into()));
        assert_eq!(entity.get(health()).as_deref(), Ok(&100.0));

        assert!(entity.remove(pos()).is_err());
        assert!(entity.has(health()));
        let h = entity.remove(health()).unwrap();
        assert_eq!(h, 100.0);
        assert!(!entity.has(health()));

        let entity = world.entity(id).unwrap();

        assert_eq!(entity.get(name()).as_deref(), Ok(&"Foo".into()));

        assert!(entity.get(pos()).is_err());
        assert!(entity.get(health()).is_err());
        assert!(!entity.has(health()));

        let mut entity = world.entity_mut(id).unwrap();

        entity.set(pos(), (0.0, 0.0)).unwrap();
        let pos = entity.entry(pos()).and_modify(|v| v.0 += 1.0).or_default();
        assert_eq!(*pos, (1.0, 0.0));
    }
}
