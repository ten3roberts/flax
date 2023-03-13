use core::mem;

use crate::{archetype::RefMut, Component, ComponentValue, Entity, World};

pub enum Entry<'a, T: ComponentValue> {
    Vacant(VacantEntry<'a, T>),
    Occupied(OccupiedEntry<'a, T>),
}

pub struct VacantEntry<'a, T: ComponentValue> {
    pub(crate) world: &'a mut World,
    pub(crate) id: Entity,
    pub(crate) component: Component<T>,
}

impl<'a, T: ComponentValue> VacantEntry<'a, T> {
    pub fn new(world: &'a mut World, id: Entity, component: Component<T>) -> Self {
        Self {
            world,
            id,
            component,
        }
    }

    pub fn insert(self, value: T) -> RefMut<'a, T> {
        let (old, loc) = self
            .world
            .set_inner(self.id, self.component, value)
            .expect("Entry is valid");
        assert!(old.is_none());
        self.world.get_mut_at(loc, self.component).unwrap()
    }
}

pub struct OccupiedEntry<'a, T: ComponentValue> {
    pub(crate) borrow: RefMut<'a, T>,
}

impl<'a, T: ComponentValue> OccupiedEntry<'a, T> {
    pub fn new(borrow: RefMut<'a, T>) -> Self {
        Self { borrow }
    }

    pub fn into_mut(self) -> RefMut<'a, T> {
        self.borrow
    }
}

impl<'a, T> Entry<'a, T>
where
    T: ComponentValue,
{
    pub fn and_modify(mut self, mut func: impl FnMut(&mut T)) -> Self {
        if let Self::Occupied(v) = &mut self {
            (func)(&mut *v.borrow)
        }

        self
    }

    /// Returns the contained component or inserts a default.
    pub fn or_insert(self, value: T) -> RefMut<'a, T> {
        match self {
            Entry::Vacant(slot) => slot.insert(value),
            Entry::Occupied(slot) => slot.into_mut(),
        }
    }

    /// Return the component in the entry or insert the default value.
    pub fn or_default(self) -> RefMut<'a, T>
    where
        T: Default,
    {
        match self {
            Entry::Vacant(slot) => slot.insert(Default::default()),
            Entry::Occupied(slot) => slot.into_mut(),
        }
    }

    /// Returns the contained component or inserts a default provided by the
    /// function.
    pub fn or_insert_with(self, func: impl FnOnce() -> T) -> RefMut<'a, T> {
        match self {
            Entry::Vacant(slot) => slot.insert((func)()),
            Entry::Occupied(slot) => slot.into_mut(),
        }
    }

    /// Replaces the current value and returns it
    pub fn set(self, value: T) -> Option<T> {
        match self {
            Entry::Vacant(slot) => {
                slot.insert(value);
                None
            }
            Entry::Occupied(mut slot) => Some(mem::replace(&mut slot.borrow, value)),
        }
    }
}
