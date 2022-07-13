use std::mem::MaybeUninit;

use atomic_refcell::AtomicRef;

use crate::{component, error::Result, Component, ComponentValue, Entity, EntityLocation, World};

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
        self.world.get_at(self.loc, self.id, component)
    }

    /// Access a component mutably
    pub fn get_mut<T: ComponentValue>(&self, component: Component<T>) -> Result<AtomicRef<T>> {
        self.world.get_at(self.loc, self.id, component)
    }

    /// Check if the entity currently has the specified component without
    /// borrowing.
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.world.archetype(self.loc.arch).has(component.id())
    }

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

    /// Returns the entity id
    pub fn id(&self) -> Entity {
        self.id
    }
}

/// Borrow all the components of an entity at once.
///
/// This is handy to borrow an entity and perform multiple operations on it
/// without mentioning the id and performing re-lookups.
pub struct EntityRef<'a> {
    pub(crate) world: &'a World,
    pub(crate) loc: EntityLocation,
    pub(crate) id: Entity,
}

impl<'a> EntityRef<'a> {
    /// Access a component
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Result<AtomicRef<T>> {
        self.world.get_at(self.loc, self.id, component)
    }

    /// Access a component mutably
    pub fn get_mut<T: ComponentValue>(&self, component: Component<T>) -> Result<AtomicRef<T>> {
        self.world.get_at(self.loc, self.id, component)
    }

    /// Check if the entity currently has the specified component without
    /// borrowing.
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.world.archetype(self.loc.arch).has(component.id())
    }
}

#[cfg(test)]
mod test {
    use crate::{component, EntityBuilder};

    use super::*;

    #[test]
    fn entity_ref() {
        component! {
            name: String,
            health: f32,
            pos: (f32, f32),
        }

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(name(), "Foo".to_string())
            .spawn(&mut world);

        let mut entity = world.entity_mut(id).unwrap();

        assert_eq!(entity.get(name()).as_deref(), Ok(&"Foo".to_string()));

        entity.set(health(), 100.0).unwrap();

        assert_eq!(entity.get(name()).as_deref(), Ok(&"Foo".to_string()));
        assert_eq!(entity.get(health()).as_deref(), Ok(&100.0));

        assert!(entity.remove(pos()).is_err());
        assert_eq!(entity.has(health()), true);
        let h = entity.remove(health()).unwrap();
        assert_eq!(h, 100.0);
        assert_eq!(entity.has(health()), false);

        let entity = world.entity(id).unwrap();

        assert_eq!(entity.get(name()).as_deref(), Ok(&"Foo".to_string()));

        assert!(entity.get(pos()).is_err());
        assert!(entity.get(health()).is_err());
        assert_eq!(entity.has(health()), false);
    }
}
