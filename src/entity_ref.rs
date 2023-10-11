use core::{
    fmt::{Debug, Display},
    mem::MaybeUninit,
};

use atomic_refcell::{AtomicRef, BorrowError, BorrowMutError};
use once_cell::unsync::OnceCell;

use crate::{
    archetype::{Archetype, RefMut, Slot},
    entity::EntityLocation,
    entry::{Entry, OccupiedEntry, VacantEntry},
    error::MissingComponent,
    format::EntityFormatter,
    name,
    query::QueryOne,
    writer::{EntityWriter, FnWriter, Missing, Replace, SingleComponentWriter, WriteDedup},
    Component, ComponentKey, ComponentValue, Entity, Fetch, RelationExt, World,
};
use crate::{RelationIter, RelationIterMut};

/// Borrow all the components of an entity at once.
///
/// This is handy to borrow an entity and perform multiple operations on it
/// without mentioning the id and performing re-lookups.
pub struct EntityRefMut<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) loc: OnceCell<EntityLocation>,
    pub(crate) id: Entity,
}

impl<'a> EntityRefMut<'a> {
    /// Access a component
    pub fn get<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> Result<AtomicRef<T>, MissingComponent> {
        self.world
            .get_at(self.loc(), component)
            .ok_or_else(|| MissingComponent {
                id: self.id,
                desc: component.desc(),
            })
    }

    /// Access a component mutably
    pub fn get_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> Result<RefMut<T>, MissingComponent> {
        self.world
            .get_mut_at(self.loc(), component)
            .ok_or_else(|| MissingComponent {
                id: self.id,
                desc: component.desc(),
            })
    }

    /// Shorthand to copy and not use a borrowing references
    pub fn get_copy<T: ComponentValue + Copy>(
        &self,
        component: Component<T>,
    ) -> Result<T, MissingComponent> {
        self.get(component).map(|v| *v)
    }

    /// Check if the entity currently has the specified component without
    /// borrowing.
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.world
            .archetypes
            .get(self.loc().arch_id)
            .has(component.key())
    }

    /// Updates a component in place
    pub fn update<T: ComponentValue, U>(
        &self,
        component: Component<T>,
        f: impl FnOnce(&mut T) -> U,
    ) -> Result<U, MissingComponent> {
        let loc = self.loc();
        let arch = self.world.archetypes.get(loc.arch_id);
        let tick = self.world.advance_change_tick();

        arch.update(loc.slot, component, FnWriter::new(f), tick)
            .ok_or(MissingComponent {
                id: self.id,
                desc: component.desc(),
            })
    }

    /// Updates a component in place
    pub fn update_dedup<T: ComponentValue + PartialEq>(
        &self,
        component: Component<T>,
        value: T,
    ) -> Result<(), MissingComponent> {
        let loc = self.loc();
        let arch = self.world.archetypes.get(loc.arch_id);
        let tick = self.world.advance_change_tick();

        arch.update(loc.slot, component, WriteDedup::new(value), tick)
            .ok_or(MissingComponent {
                id: self.id,
                desc: component.desc(),
            })
    }

    pub fn query_one<Q: Fetch<'a>>(&self, query: Q) -> Option<QueryOne<'a, Q>> {
        todo!()
    }

    /// Attempt concurrently access a component mutably using and fail if the component is already borrowed
    pub fn try_get<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> core::result::Result<Option<AtomicRef<T>>, BorrowError> {
        self.world.try_get_at(self.loc(), component)
    }

    /// Attempt to concurrently access a component mutably using and fail if the component is already borrowed
    pub fn try_get_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> core::result::Result<Option<RefMut<T>>, BorrowMutError> {
        self.world.try_get_mut_at(self.loc(), component)
    }

    #[inline]
    fn loc(&self) -> EntityLocation {
        *self
            .loc
            .get_or_init(|| self.world.location(self.id).unwrap())
    }

    /// Returns all relations to other entities of the specified kind
    pub fn relations<T: ComponentValue>(&self, relation: impl RelationExt<T>) -> RelationIter<T> {
        let (_, loc, arch) = self.parts();
        RelationIter::new(relation, arch, loc.slot)
    }

    /// Returns all relations to other entities of the specified kind
    pub fn relations_mut<T: ComponentValue>(
        &self,
        relation: impl RelationExt<T>,
    ) -> RelationIterMut<T> {
        let (world, loc, arch) = self.parts();
        RelationIterMut::new(relation, arch, loc.slot, world.advance_change_tick())
    }

    /// Set a component for the entity
    pub fn set<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> Option<T> {
        self.set_with_writer(SingleComponentWriter::new(
            component.desc(),
            Replace::new(value),
        ))
        .left()
    }

    /// Set a component for the entity only if it is missing.
    ///
    /// Does not disturb or generate a change event if the component is present
    pub fn set_missing<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> bool {
        self.set_with_writer(SingleComponentWriter::new(
            component.desc(),
            Missing { value },
        ))
        .is_right()
    }

    /// Set a component for the entity.
    ///
    /// Does not trigger a modification event if the value is the same
    pub fn set_dedup<T: ComponentValue + PartialEq>(&mut self, component: Component<T>, value: T) {
        self.set_with_writer(SingleComponentWriter::new(
            component.desc(),
            WriteDedup::new(value),
        ));
    }

    /// Set a component for the entity
    pub(crate) fn set_with_writer<W: EntityWriter>(&mut self, writer: W) -> W::Output {
        let (loc, res) = self.world.set_with_writer(self.id, writer).unwrap();
        self.loc = OnceCell::with_value(loc);
        res
    }

    /// Remove a component
    pub fn remove<T: ComponentValue>(
        &mut self,
        component: Component<T>,
    ) -> Result<T, MissingComponent> {
        let mut res: MaybeUninit<T> = MaybeUninit::uninit();
        let (old, loc) = unsafe {
            let loc = self
                .world
                .remove_inner(self.id, component.desc(), |ptr| {
                    res.write(ptr.cast::<T>().read());
                })
                .map_err(|v| v.try_into_missing_component().unwrap())?;
            (res.assume_init(), loc)
        };

        self.loc = OnceCell::with_value(loc);
        Ok(old)
    }

    /// Retain only the components specified by the predicate
    pub fn retain(&mut self, f: impl FnMut(ComponentKey) -> bool) {
        self.loc = OnceCell::with_value(self.world.retain_entity_components(self.id, self.loc(), f))
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
            let loc = self.loc();
            Entry::Occupied(OccupiedEntry {
                borrow: self.world.get_mut_at(loc, component).unwrap(),
            })
        } else {
            Entry::Vacant(VacantEntry {
                world: self.world,
                id: self.id,
                component,
            })
        }
    }

    /// Non consuming version of [`Self::entry`]
    pub fn entry_ref<T: ComponentValue>(&mut self, component: Component<T>) -> Entry<T> {
        if self.has(component) {
            let loc = self.loc();
            Entry::Occupied(OccupiedEntry {
                borrow: self.world.get_mut_at(loc, component).unwrap(),
            })
        } else {
            self.loc.take();
            Entry::Vacant(VacantEntry {
                world: self.world,
                id: self.id,
                component,
            })
        }
    }

    fn parts(&self) -> (&World, EntityLocation, &Archetype) {
        let loc = self.loc();
        let arch = self.world.archetypes.get(loc.arch_id);
        (self.world, loc, arch)
    }

    /// Non consuming version of [`Self::downgrade`]
    #[inline]
    pub fn downgrade_ref(&self) -> EntityRef {
        let loc = self.loc();
        EntityRef {
            arch: self.world.archetypes.get(loc.arch_id),
            slot: loc.slot,
            id: self.id,
            world: self.world,
        }
    }

    /// Convert the [`EntityRefMut`] into a [`EntityRef`]
    #[inline]
    pub fn downgrade(self) -> EntityRef<'a> {
        let loc = self.loc();
        EntityRef {
            arch: self.world.archetypes.get(loc.arch_id),
            slot: loc.slot,
            id: self.id,
            world: self.world,
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
    pub(crate) arch: &'a Archetype,
    pub(crate) slot: Slot,
    pub(crate) id: Entity,
}

impl<'a> EntityRef<'a> {
    /// Access a component
    pub fn get<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> Result<AtomicRef<'a, T>, MissingComponent> {
        self.arch
            .get(self.slot, component)
            .ok_or_else(|| MissingComponent {
                id: self.id,
                desc: component.desc(),
            })
    }

    /// Access a component mutably
    pub fn get_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> Result<RefMut<'a, T>, MissingComponent> {
        self.arch
            .get_mut(self.slot, component, self.world.advance_change_tick())
            .ok_or_else(|| MissingComponent {
                id: self.id,
                desc: component.desc(),
            })
    }

    /// Shorthand to copy and not use a borrowing references
    pub fn get_copy<T: ComponentValue + Copy>(
        &self,
        component: Component<T>,
    ) -> Result<T, MissingComponent> {
        self.get(component).map(|v| *v)
    }

    /// Check if the entity currently has the specified component without
    /// borrowing.
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.arch.has(component.key())
    }

    /// Updates a component in place
    pub fn update<T: ComponentValue, U>(
        &self,
        component: Component<T>,
        f: impl FnOnce(&mut T) -> U,
    ) -> Option<U> {
        let change_tick = self.world.advance_change_tick();

        self.arch
            .update(self.slot, component, FnWriter::new(f), change_tick)
    }

    /// Updates a component in place
    pub fn update_dedup<T: ComponentValue + PartialEq>(
        &self,
        component: Component<T>,
        value: T,
    ) -> Option<()> {
        let tick = self.world.advance_change_tick();

        self.arch
            .update(self.slot, component, WriteDedup::new(value), tick)
    }

    /// Attempt concurrently access a component mutably using and fail if the component is already borrowed
    pub fn try_get<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> core::result::Result<Option<AtomicRef<T>>, BorrowError> {
        self.arch.try_get(self.slot, component)
    }

    /// Attempt to concurrently access a component mutably using and fail if the component is already borrowed
    pub fn try_get_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> core::result::Result<Option<RefMut<T>>, BorrowMutError> {
        self.arch
            .try_get_mut(self.slot, component, self.world.advance_change_tick())
    }

    /// Returns all relations to other entities of the specified kind
    #[inline]
    pub fn relations<T: ComponentValue>(
        &self,
        relation: impl RelationExt<T>,
    ) -> RelationIter<'a, T> {
        RelationIter::new(relation, self.arch, self.slot)
    }

    /// Returns all relations to other entities of the specified kind
    #[inline]
    pub fn relations_mut<T: ComponentValue>(
        &self,
        relation: impl RelationExt<T>,
    ) -> RelationIterMut<'a, T> {
        RelationIterMut::new(
            relation,
            self.arch,
            self.slot,
            self.world.advance_change_tick(),
        )
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
            arch: self.arch,
            slot: self.slot,
            id: self.id,
        }
        .fmt(f)
    }
}

impl<'a> Debug for EntityRefMut<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let loc = self.loc();

        let arch = self.world.archetypes.get(loc.arch_id);

        EntityFormatter {
            world: self.world,
            id: self.id,
            slot: loc.slot,
            arch,
        }
        .fmt(f)
    }
}

impl Display for EntityRef<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let id = self.id();
        if let Some(name) = self.try_get(name()).ok().flatten() {
            write!(f, "{} {id}", &*name)
        } else {
            write!(f, "{id}")
        }
    }
}

impl Display for EntityRefMut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let id = self.id();
        if let Some(name) = self.try_get(name()).ok().flatten() {
            write!(f, "{} {id}", &*name)
        } else {
            write!(f, "{id}")
        }
    }
}

#[cfg(test)]
mod test {

    use crate::{component, components::name, is_static, EntityBuilder, FetchExt, Query};

    use super::*;

    #[test]
    fn spawn_ref() {
        let mut world = World::new();

        let mut entity = world.spawn_ref();

        let id = entity.id();

        assert_eq!(entity.entry_ref(name()).set("Bar".into()), None);
        assert_eq!(
            entity.entry_ref(name()).set("Foo".into()),
            Some("Bar".into())
        );

        assert_eq!(
            entity.world().get_mut(id, name()).as_deref(),
            Ok(&"Foo".into())
        );

        let entity = entity.downgrade();
        let res = entity.get(is_static());

        assert_eq!(
            res.as_deref(),
            Err(&MissingComponent {
                id,
                desc: is_static().desc()
            })
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

        entity.set(health(), 100.0);
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

        entity.set(pos(), (0.0, 0.0));
        let pos = entity.entry(pos()).and_modify(|v| v.0 += 1.0).or_default();

        assert_eq!(*pos, (1.0, 0.0));
    }

    #[test]
    fn display_borrowed() {
        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(name(), "Foo".into())
            .spawn(&mut world);

        let entity = world.entity(id).unwrap();

        assert_eq!(alloc::format!("{}", entity), alloc::format!("Foo {id}"));

        let _name = world.get_mut(id, name()).unwrap();

        assert_eq!(alloc::format!("{}", entity), alloc::format!("{id}"));
    }

    #[test]
    fn display_borrowed_mut() {
        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(name(), "Foo".into())
            .spawn(&mut world);

        let entity = world.entity_mut(id).unwrap();

        assert_eq!(alloc::format!("{}", entity), alloc::format!("Foo {id}"));

        let _name = entity.get_mut(name()).unwrap();

        assert_eq!(alloc::format!("{}", entity), alloc::format!("{id}"));
    }

    #[test]
    fn update() {
        use alloc::string::{String, ToString};
        component! {
            a: String,
            b: String,
        }

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(a(), "Foo".into())
            .spawn(&mut world);

        let entity = world.entity(id).unwrap();

        let mut query = Query::new(a().modified().cloned());
        assert_eq!(query.collect_vec(&world), ["Foo"]);
        assert_eq!(entity.update(a(), |v| v.push_str("Bar")), Some(()));
        assert_eq!(query.collect_vec(&world), ["FooBar"]);
        assert_eq!(entity.update(b(), |v| v.push('_')), None);
        assert!(query.collect_vec(&world).is_empty());

        assert_eq!(entity.get(a()).as_deref(), Ok(&"FooBar".to_string()));
        assert!(entity.get(b()).is_err());
    }

    #[test]
    fn update_mut() {
        use alloc::string::{String, ToString};
        component! {
            a: String,
            b: String,
        }

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(a(), "Foo".into())
            .spawn(&mut world);

        let entity = world.entity_mut(id).unwrap();

        assert_eq!(entity.update(a(), |v| v.push_str("Bar")), Ok(()));
        assert_eq!(
            entity.update(b(), |v| v.push('_')),
            Err(MissingComponent {
                id,
                desc: b().desc()
            })
        );

        assert_eq!(entity.get(a()).as_deref(), Ok(&"FooBar".to_string()));
        assert!(entity.get(b()).is_err());
    }

    #[test]
    fn set_dedup() {
        use alloc::string::String;
        component! {
            a: String,
        }

        let mut world = World::new();

        let mut query = Query::new(a().modified().cloned());

        let id = EntityBuilder::new()
            .set(a(), "Foo".into())
            .spawn(&mut world);

        assert_eq!(query.collect_vec(&world), ["Foo"]);

        let mut entity = world.entity_mut(id).unwrap();
        entity.set_dedup(a(), "Foo".into());

        assert!(query.collect_vec(&world).is_empty());
        let mut entity = world.entity_mut(id).unwrap();
        entity.set_dedup(a(), "Bar".into());

        assert_eq!(query.collect_vec(&world), ["Bar"]);
    }

    #[test]
    fn set_missing() {
        use alloc::string::String;
        use alloc::string::ToString;

        component! {
            a: String,
        }

        let mut world = World::new();

        let mut query = Query::new((a().modified().satisfied(), a().cloned()));

        let id = EntityBuilder::new().spawn(&mut world);

        assert!(query.collect_vec(&world).is_empty());

        let mut entity = world.entity_mut(id).unwrap();
        assert!(entity.set_missing(a(), "Foo".into()));

        assert_eq!(query.collect_vec(&world), [(true, "Foo".to_string())]);
        assert_eq!(query.collect_vec(&world), [(false, "Foo".to_string())]);

        let mut entity = world.entity_mut(id).unwrap();
        assert!(!entity.set_missing(a(), "Bar".into()));

        assert_eq!(query.collect_vec(&world), [(false, "Foo".to_string())]);
    }

    #[test]
    fn update_dedup() {
        use alloc::string::String;
        component! {
            a: String,
        }

        let mut world = World::new();

        let mut query = Query::new(a().modified().cloned());

        let id = EntityBuilder::new()
            .set(a(), "Foo".into())
            .spawn(&mut world);

        assert_eq!(query.collect_vec(&world), ["Foo"]);

        let entity = world.entity_mut(id).unwrap();
        let _ = entity.update_dedup(a(), "Foo".into());

        assert!(query.collect_vec(&world).is_empty());

        let entity = world.entity_mut(id).unwrap();
        let _ = entity.update_dedup(a(), "Bar".into());

        assert_eq!(query.collect_vec(&world), ["Bar"]);
    }
}
