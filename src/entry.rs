use core::mem;

use crate::{
    archetype::RefMut,
    writer::{Replace, SingleComponentWriter},
    Component, ComponentValue, Entity, World,
};

/// Entry like api for an entity's component
pub enum Entry<'a, T: ComponentValue> {
    /// A vacant entry
    Vacant(VacantEntry<'a, T>),
    /// An occupied entry
    Occupied(OccupiedEntry<'a, T>),
}

/// A view into a vacant component entry
pub struct VacantEntry<'a, T: ComponentValue> {
    pub(crate) world: &'a mut World,
    pub(crate) id: Entity,
    pub(crate) component: Component<T>,
}

impl<'a, T: ComponentValue> VacantEntry<'a, T> {
    /// Insert a value into the entry, returning a mutable reference to it
    pub fn insert(self, value: T) -> RefMut<'a, T> {
        let loc = self
            .world
            .set_with_writer(
                self.id,
                SingleComponentWriter::new(
                    self.component.info(),
                    Replace {
                        value,
                        output: &mut None,
                    },
                ),
            )
            .expect("Entry is valid");

        self.world.get_mut_at(loc, self.component).unwrap()
    }
}

/// A view into an occupied component entry
pub struct OccupiedEntry<'a, T: ComponentValue> {
    pub(crate) borrow: RefMut<'a, T>,
}

impl<'a, T: ComponentValue> OccupiedEntry<'a, T> {
    /// Convert the entry into a mutable reference
    pub fn into_mut(self) -> RefMut<'a, T> {
        self.borrow
    }
}

impl<'a, T> Entry<'a, T>
where
    T: ComponentValue,
{
    /// Mutate the value in place
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
