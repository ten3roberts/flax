use core::{
    fmt::{self, Debug, Display, Formatter},
    marker::PhantomData,
    sync::atomic::AtomicU32,
};

#[cfg(feature = "serde")]
use serde::{
    de::{Error, Visitor},
    ser::SerializeTupleStruct,
    Deserialize, Serialize,
};

use crate::{
    archetype::ComponentInfo,
    buffer::ComponentBuffer,
    entity::EntityKind,
    filter::ChangeFilter,
    filter::With,
    filter::Without,
    filter::{RemovedFilter, WithRelation, WithoutRelation},
    ChangeKind, Entity, MetaData, Mutable,
};

/// Trait alias for a 'static + Send + Sync type which can be used as a
/// component.
pub trait ComponentValue: Send + Sync + 'static {}
impl<T> ComponentValue for T where T: Send + Sync + 'static {}

/// A unique component identifier
/// Is not stable between executions, and should as such not be used for
/// execution.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentKey {
    pub(crate) id: Entity,
    /// The object entity if the component is a pair
    pub(crate) object: Option<Entity>,
}

#[cfg(feature = "serde")]
impl Serialize for ComponentKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_tuple_struct("ComponentId", 2)?;
        seq.serialize_field(&self.id)?;
        seq.serialize_field(&self.object)?;

        seq.end()
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for ComponentKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ComponentIdVisitor;
        impl<'de> Visitor<'de> for ComponentIdVisitor {
            type Value = ComponentKey;

            fn expecting(
                &self,
                formatter: &mut smallvec::alloc::fmt::Formatter,
            ) -> smallvec::alloc::fmt::Result {
                write!(
                    formatter,
                    "A tuple of a component id and optional relation object"
                )
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let id = seq
                    .next_element()?
                    .ok_or_else(|| Error::invalid_length(0, &self))?;
                let object = seq
                    .next_element()?
                    .ok_or_else(|| Error::invalid_length(1, &self))?;

                Ok(ComponentKey::new(id, object))
            }
        }

        deserializer.deserialize_tuple_struct("ComponentId", 2, ComponentIdVisitor)
    }
}

impl ComponentKey {
    /// Returns true if the component is a relation
    pub fn is_relation(&self) -> bool {
        self.object.is_some()
    }

    pub(crate) fn new(id: Entity, object: Option<Entity>) -> Self {
        Self { id, object }
    }
}

impl Display for ComponentKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Debug for ComponentKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self.object {
            Some(s) => write!(f, "{}({s})", self.id),
            None => Debug::fmt(&self.id, f),
        }
    }
}

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
    fn with_relation(self) -> WithRelation;
    /// Construct a new filter yielding entities without this kind of relation
    fn without_relation(self) -> WithoutRelation;
}

impl<T: ComponentValue> RelationExt<T> for Component<T> {
    fn of(&self, object: Entity) -> Component<T> {
        Self {
            key: ComponentKey::new(self.key.id, Some(object)),
            ..*self
        }
    }

    fn with_relation(self) -> WithRelation {
        WithRelation {
            relation: self.id(),
            name: self.name(),
        }
    }

    fn without_relation(self) -> WithoutRelation {
        WithoutRelation {
            relation: self.id(),
            name: self.name(),
        }
    }
}

impl<T, F> RelationExt<T> for F
where
    F: Fn(Entity) -> Component<T>,
    T: ComponentValue,
{
    fn of(&self, object: Entity) -> Component<T> {
        (self)(object)
    }

    fn with_relation(self) -> WithRelation {
        let c = self(dummy());
        WithRelation {
            relation: c.id(),
            name: c.name(),
        }
    }

    fn without_relation(self) -> WithoutRelation {
        let c = self(dummy());
        WithoutRelation {
            relation: c.id(),
            name: c.name(),
        }
    }
}

crate::component! {
    pub(crate) dummy,
}

/// Defines a strongly typed component
pub struct Component<T> {
    key: ComponentKey,
    name: &'static str,
    marker: PhantomData<T>,

    /// A metadata is a component which is attached to the component, such as
    /// metadata or name
    meta: fn(ComponentInfo) -> ComponentBuffer,
}

impl<T: ComponentValue> Eq for Component<T> {}

impl<T: ComponentValue> PartialEq for Component<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: ComponentValue> Copy for Component<T> {}

impl<T: ComponentValue> Clone for Component<T> {
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            name: self.name,
            marker: PhantomData,
            meta: self.meta,
        }
    }
}

impl<T: ComponentValue> fmt::Debug for Component<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Component").field("id", &self.key).finish()
    }
}

impl<T: ComponentValue> Display for Component<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.name, self.key)
    }
}

impl<T: ComponentValue> Component<T> {
    /// Create a new component given a unique id and name.
    ///
    /// # Safety
    /// The constructed component can not be of a different type, name or meta
    /// than any existing component of the same id
    pub(crate) unsafe fn from_raw_id(
        id: ComponentKey,
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
    ) -> Self {
        Self {
            key: id,
            name,
            marker: PhantomData,
            meta,
        }
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
        unsafe { Self::from_raw_id(ComponentKey::new(id, None), name, meta) }
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
    pub fn key(&self) -> ComponentKey {
        self.key
    }

    /// Get the component's base id.
    /// This is the id without any relation object
    #[inline(always)]
    pub fn id(&self) -> Entity {
        self.key.id
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
        ChangeFilter::new(self, ChangeKind::Modified)
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
            component: self.key(),
            name: self.name(),
        }
    }

    /// Construct a new filter yielding entities with this component.
    pub fn with(self) -> With {
        With {
            component: self.key(),
            name: self.name(),
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
        v.key().id
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
        let _c_foo = foo();
        // eprintln!("Foo: {c_foo:?}");
        // eprintln!("Bar: {:?}", bar().id());
        assert_ne!(foo().key(), bar().key());
        assert_eq!(foo(), foo());
    }
}
