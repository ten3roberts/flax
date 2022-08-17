use std::{fmt::Display, marker::PhantomData, sync::atomic::AtomicU32};

use crate::{
    archetype::ComponentInfo, wildcard, ComponentBuffer, Entity, EntityKind, InsertedFilter,
    MetaData, ModifiedFilter, Mutable, Relation, RemovedFilter, With, Without,
};

/// Trait alias for a 'static + Send + Sync type which can be used as a
/// component.
pub trait ComponentValue: Send + Sync + 'static {}

/// A unique component identifier
/// Is not stable between executions, and should as such not be used for
/// execution.
pub type ComponentId = Entity;

impl<T> ComponentValue for T where T: Send + Sync + 'static {}

/// Defines a strongly typed component
pub struct Component<T> {
    id: ComponentId,
    name: &'static str,
    marker: PhantomData<T>,

    /// A metadata is a component which is attached to the component, such as
    /// metadata or name
    meta: fn(ComponentInfo) -> ComponentBuffer,
}

impl<T: ComponentValue> Eq for Component<T> {}

impl<T: ComponentValue> PartialEq for Component<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: ComponentValue> Copy for Component<T> {}

impl<T: ComponentValue> Clone for Component<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name,
            marker: PhantomData,
            meta: self.meta,
        }
    }
}

impl<T: ComponentValue> std::fmt::Debug for Component<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Component").field("id", &self.id).finish()
    }
}

impl<T: ComponentValue> Display for Component<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name, self.id)
    }
}

impl<T: ComponentValue> Component<T> {
    /// Create a new component given a unique id and name.
    ///
    /// *SAFETY*: The id must not be used by anything else
    pub fn new(
        id: ComponentId,
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
    ) -> Self {
        Self {
            id,
            name,
            marker: PhantomData,
            meta,
        }
    }

    /// Creates a new relation component with the specified object entity
    pub fn new_pair(
        id: ComponentId,
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
        object: Entity,
    ) -> Self {
        Self {
            id: Entity::pair(id, object),
            name,
            marker: PhantomData,
            meta,
        }
    }

    #[doc(hidden)]
    pub fn static_init(
        id: &AtomicU32,
        name: &'static str,
        kind: EntityKind,
        meta: fn(ComponentInfo) -> ComponentBuffer,
    ) -> Self {
        let id = Entity::static_init(id, kind);

        Self::new(id, name, meta)
    }

    /// Attaches a function to generate component metadata
    pub fn set_meta(&mut self, meta: fn(ComponentInfo) -> ComponentBuffer) {
        self.meta = meta
    }

    /// Returns all metadata components
    pub fn get_meta(&self) -> ComponentBuffer {
        (self.meta)(self.info())
    }

    /// Get the component's id.
    #[inline(always)]
    pub fn id(&self) -> ComponentId {
        self.id
    }

    /// Returns the type erased component info
    pub fn info(self) -> ComponentInfo {
        ComponentInfo::of(self)
    }

    /// Transform this into a mutable fetch
    pub fn as_mut(self) -> Mutable<T> {
        Mutable(self)
    }

    /// Construct a fine grained change detection filter.
    pub fn modified(self) -> ModifiedFilter {
        ModifiedFilter::new(self.id())
    }

    /// Construct a fine grained insert detection filter.
    pub fn inserted(self) -> InsertedFilter {
        InsertedFilter::new(self.id())
    }

    /// Construct a fine grained component remove detection filter.
    pub fn removed(self) -> RemovedFilter {
        RemovedFilter::new(self.id())
    }

    /// Construct a new filter yielding entities without this component.
    pub fn without(self) -> Without {
        Without {
            component: self.id(),
        }
    }

    /// Construct a new filter yielding entities with this component.
    pub fn with(self) -> With {
        With {
            component: self.id(),
        }
    }

    /// Construct a fetch which will visit the `index` relation of this
    /// component type.
    /// The index is used since there may be multiple distinct relations of the
    /// same component types
    pub fn relation(self, index: usize) -> Relation<T> {
        Relation::new(self, index)
    }

    /// Get the component's name.
    #[must_use]
    #[inline(always)]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the component metadata fn
    pub fn meta(&self) -> fn(ComponentInfo) -> ComponentBuffer {
        self.meta
    }
}

impl<T: ComponentValue> MetaData<T> for Component<T> {
    fn attach(info: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(crate::components::is_component(), info);
    }
}

impl<T: ComponentValue> From<Component<T>> for Entity {
    fn from(v: Component<T>) -> Self {
        v.id()
    }
}

#[cfg(test)]
mod tests {
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
