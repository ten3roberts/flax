use std::{
    fmt::Display,
    marker::PhantomData,
    sync::atomic::{
        AtomicU32,
        Ordering::{Acquire, Relaxed},
    },
};

use crate::{
    archetype::ComponentInfo,
    entity::{EntityIndex, EntityKind},
    Entity,
};

pub trait ComponentValue: Send + Sync + 'static {}
pub type ComponentId = Entity;

impl<T> ComponentValue for T where T: Send + Sync + 'static {}

/// Defines a strongly typed component
pub struct Component<T> {
    id: ComponentId,
    name: &'static str,
    marker: PhantomData<T>,
}

impl<T> Eq for Component<T> {}

impl<T> PartialEq for Component<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Copy for Component<T> {}

impl<T> Clone for Component<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name,
            marker: PhantomData,
        }
    }
}

impl<T: ComponentValue> std::fmt::Debug for Component<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Component").field("id", &self.id).finish()
    }
}

impl<T> Display for Component<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name, self.id)
    }
}

impl<T: ComponentValue> Component<T> {
    pub fn new(id: ComponentId, name: &'static str) -> Self {
        Self {
            id,
            name,
            marker: PhantomData,
        }
    }

    pub fn static_init(id: &AtomicU32, name: &'static str) -> Self {
        let index = match id.fetch_update(Acquire, Relaxed, |v| {
            if v != 0 {
                None
            } else {
                Some(
                    ComponentId::acquire_static_id(EntityKind::empty())
                        .index()
                        .get(),
                )
            }
        }) {
            Ok(_) => id.load(Acquire),
            Err(old) => old,
        };

        Self::new(
            Entity::from_parts(
                EntityIndex::new(index).unwrap(),
                0,
                EntityKind::COMPONENT | EntityKind::STATIC,
            ),
            name,
        )
    }

    /// Get the component's id.
    #[must_use]
    #[inline(always)]
    pub fn id(&self) -> ComponentId {
        self.id
    }

    pub fn info(self) -> ComponentInfo {
        ComponentInfo::of(self)
    }

    /// Get the component's name.
    #[must_use]
    #[inline(always)]
    pub fn name(&self) -> &'static str {
        self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    component! {
        foo: i32,
        bar: f32,
    }

    #[test]
    fn component_ids() {
        let c_foo = foo();
        eprintln!("Foo: {c_foo:?}");
        eprintln!("Bar: {:?}", bar().id());
        assert_ne!(foo().id(), bar().id());
        assert_eq!(foo(), foo());
    }
}
