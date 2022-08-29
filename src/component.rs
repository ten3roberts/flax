use std::{fmt::Display, marker::PhantomData, sync::atomic::AtomicU32};

use crate::{
    archetype::ComponentInfo, buffer::ComponentBuffer, entity::wildcard, entity::EntityKind,
    filter::ChangeFilter, filter::RemovedFilter, filter::With, filter::Without, ChangeKind, Entity,
    MetaData, Mutable,
};

/// Trait alias for a 'static + Send + Sync type which can be used as a
/// component.
pub trait ComponentValue: Send + Sync + 'static {}
impl<T> ComponentValue for T where T: Send + Sync + 'static {}

/// A unique component identifier
/// Is not stable between executions, and should as such not be used for
/// execution.
pub type ComponentId = Entity;

/// Type alias for a function which instantiates a component
pub type ComponentFn<T> = fn() -> Component<T>;

/// Type alias for a function which instantiates a relation with the specified
/// object
pub type RelationFn<T> = fn(object: Entity) -> Component<T>;

/// Relation helper trait
pub trait RelationExt<T>
where
    T: ComponentValue,
{
    /// Instantiate the relation
    fn of(&self, object: Entity) -> Component<T>;
    /// Construct a new filter yielding entities with this kind of relation
    fn with(self) -> With;
    /// Construct a new filter yielding entities without this kind of relation
    fn without(self) -> Without;
}

impl<T, F> RelationExt<T> for F
where
    F: Fn(Entity) -> Component<T>,
    T: ComponentValue,
{
    fn of(&self, object: Entity) -> Component<T> {
        (self)(object)
    }

    fn with(self) -> With {
        With {
            component: self(wildcard()).id(),
        }
    }

    fn without(self) -> Without {
        Without {
            component: self(wildcard()).id(),
        }
    }
}

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
    /// # Safety
    /// The constructed component can not be of a different type, name or meta
    /// than any existing component of the same id
    pub(crate) unsafe fn from_raw_id(
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
    /// # Safety
    /// See: [Component::from_raw_id]
    pub(crate) unsafe fn from_pair(
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
    pub fn static_init_pair(
        id: &AtomicU32,
        kind: EntityKind,
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
        object: Entity,
    ) -> Self {
        let id = Entity::static_init(id, kind);

        unsafe { Self::from_pair(id, name, meta, object) }
    }

    #[doc(hidden)]
    pub fn static_init(
        id: &AtomicU32,
        kind: EntityKind,
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
    ) -> Self {
        let id = Entity::static_init(id, kind);

        // Safety
        // The id is new
        unsafe { Self::from_raw_id(id, name, meta) }
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
    pub fn modified(self) -> ChangeFilter<T> {
        ChangeFilter::new(self, ChangeKind::Inserted)
    }

    /// Construct a fine grained insert detection filter.
    pub fn inserted(self) -> ChangeFilter<T> {
        ChangeFilter::new(self, ChangeKind::Inserted)
    }

    /// Construct a fine grained component remove detection filter.
    pub fn removed(self) -> RemovedFilter<T> {
        RemovedFilter::new(self)
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
