use core::{
    alloc::Layout,
    any::TypeId,
    fmt::{self, Debug, Display, Formatter},
    marker::PhantomData,
    ptr,
    sync::atomic::AtomicU32,
};

#[cfg(feature = "serde")]
use serde::{
    de::{Error, Visitor},
    ser::SerializeTupleStruct,
    Deserialize, Serialize,
};

use crate::{
    archetype::ChangeKind,
    buffer::ComponentBuffer,
    entity::EntityKind,
    fetch::MaybeMut,
    filter::{ChangeFilter, With, WithRelation, Without, WithoutRelation},
    metadata::Metadata,
    relation::RelationExt,
    vtable::{ComponentVTable, UntypedVTable},
    Entity, Mutable,
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
    /// The object entity if the component is a relation
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
    #[inline]
    pub fn is_relation(&self) -> bool {
        self.object.is_some()
    }

    pub(crate) fn new(id: Entity, object: Option<Entity>) -> Self {
        Self { id, object }
    }

    #[inline]
    /// Returns the object of the relation
    pub fn object(&self) -> Option<Entity> {
        self.object
    }

    #[inline]
    /// Returns the component id
    pub fn id(&self) -> Entity {
        self.id
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

crate::component! {
    pub(crate) dummy,
}

/// Defines a strongly typed component.
///
/// Implements a *read only* fetch when used as part of a query
///
/// Use `.as_mut()` to acquire a *mutable* fetch.
pub struct Component<T> {
    key: ComponentKey,
    marker: PhantomData<T>,

    pub(crate) vtable: &'static UntypedVTable,
}

impl<T> Eq for Component<T> {}

impl<T> PartialEq for Component<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T> Copy for Component<T> {}

impl<T> Clone for Component<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> fmt::Debug for Component<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.key.object {
            Some(object) => write!(f, "{}({}) {}", self.vtable.name, object, self.key.id()),
            None => write!(f, "{} {}", self.vtable.name, self.key.id),
        }
    }
}

impl<T> Display for Component<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.key.object {
            Some(object) => write!(f, "{}({}) {}", self.vtable.name, object, self.key.id()),
            None => write!(f, "{} {}", self.vtable.name, self.key.id),
        }
    }
}

impl<T: ComponentValue> Component<T> {
    pub(crate) fn new(key: ComponentKey, vtable: &'static ComponentVTable<T>) -> Self {
        Self {
            key,
            marker: PhantomData,
            vtable: vtable.erase(),
        }
    }
    /// Creates a new component from the given untyped vtable
    ///
    /// # Panics
    /// If the types do not match
    pub(crate) fn from_raw_parts(key: ComponentKey, vtable: &'static UntypedVTable) -> Self {
        if !vtable.is::<T>() {
            panic!("Mismatched type");
        }

        Self {
            key,
            marker: PhantomData,
            vtable,
        }
    }

    #[doc(hidden)]
    pub fn static_init(
        id: &AtomicU32,
        kind: EntityKind,
        vtable: &'static ComponentVTable<T>,
    ) -> Self {
        let id = Entity::static_init(id, kind);

        Self {
            key: ComponentKey::new(id, None),
            vtable,
            marker: PhantomData,
        }
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

    /// Returns the type erased component description
    pub fn desc(self) -> ComponentDesc {
        ComponentDesc::of(self)
    }

    /// Transform this into a mutable fetch
    pub const fn as_mut(self) -> Mutable<T> {
        Mutable(self)
    }

    /// Transform this into a (maybe) mutable fetch
    pub fn maybe_mut(self) -> MaybeMut<T> {
        MaybeMut(self)
    }

    /// Construct a fine grained change detection filter.
    ///
    /// Prefer [`TransformFetch`](crate::fetch::TransformFetch) if not in a const context
    pub fn into_change_filter(self, kind: ChangeKind) -> ChangeFilter<T> {
        ChangeFilter::new(self, kind)
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
        self.vtable.name
    }

    /// Returns all metadata components
    pub fn get_meta(&self) -> ComponentBuffer {
        self.vtable.meta.get(self.desc())
    }
}

impl<T: ComponentValue> Metadata<T> for Component<T> {
    fn attach(desc: ComponentDesc, buffer: &mut ComponentBuffer) {
        buffer.set(crate::components::component_info(), desc);
    }
}

impl<T: ComponentValue> From<Component<T>> for Entity {
    fn from(v: Component<T>) -> Self {
        v.key().id
    }
}

impl<T: ComponentValue> RelationExt<T> for Component<T> {
    fn id(&self) -> Entity {
        self.key().id
    }

    fn of(&self, object: Entity) -> Component<T> {
        Self {
            key: ComponentKey::new(self.key().id, Some(object)),
            ..*self
        }
    }

    #[inline]
    fn with_relation(self) -> WithRelation {
        WithRelation {
            relation: self.id(),
            name: self.name(),
        }
    }

    #[inline]
    fn without_relation(self) -> WithoutRelation {
        WithoutRelation {
            relation: self.id(),
            name: self.name(),
        }
    }

    fn vtable(&self) -> &'static UntypedVTable {
        self.vtable
    }
}

/// Represents a type erased component along with its memory layout and drop fn.
#[derive(Clone, Copy)]
pub struct ComponentDesc {
    pub(crate) key: ComponentKey,
    pub(crate) vtable: &'static UntypedVTable,
}

impl Eq for ComponentDesc {}
impl PartialEq for ComponentDesc {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && ptr::eq(self.vtable, other.vtable)
    }
}

impl core::fmt::Debug for ComponentDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.key.object {
            Some(object) => write!(f, "{}({}) {}", self.vtable.name, object, self.key.id()),
            None => write!(f, "{} {}", self.vtable.name, self.key.id),
        }
    }
}

impl<T: ComponentValue> From<Component<T>> for ComponentDesc {
    fn from(v: Component<T>) -> Self {
        ComponentDesc::of(v)
    }
}

impl PartialOrd for ComponentDesc {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ComponentDesc {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

impl ComponentDesc {
    /// Convert back to a typed form
    ///
    /// # Panics
    /// If the types do not match
    #[inline]
    pub fn downcast<T: ComponentValue>(self) -> Component<T> {
        Component::from_raw_parts(self.key, self.vtable)
    }

    /// Returns the component description of a types component
    pub fn of<T: ComponentValue>(component: Component<T>) -> Self {
        Self {
            key: component.key(),
            vtable: component.vtable,
        }
    }

    #[inline]
    pub(crate) fn is<T: 'static>(&self) -> bool {
        (self.vtable.type_id)() == TypeId::of::<T>()
    }

    #[inline]
    pub(crate) fn size(&self) -> usize {
        self.vtable.layout.size()
    }

    /// Returns the component name
    #[inline]
    pub fn name(&self) -> &'static str {
        self.vtable.name
    }

    /// Returns the component id
    #[inline(always)]
    pub fn key(&self) -> ComponentKey {
        self.key
    }

    #[inline]
    pub(crate) fn align(&self) -> usize {
        self.vtable.layout.align()
    }

    #[inline]
    pub(crate) unsafe fn drop(&self, ptr: *mut u8) {
        (self.vtable.drop)(ptr)
    }

    #[inline]
    pub(crate) fn layout(&self) -> Layout {
        self.vtable.layout
    }

    #[inline]
    /// Returns the type id of the component
    pub fn type_id(&self) -> TypeId {
        (self.vtable.type_id)()
    }

    #[inline]
    /// Returns the type name of the component
    pub fn type_name(&self) -> &'static str {
        (self.vtable.type_name)()
    }

    #[inline]
    pub(crate) fn is_relation(&self) -> bool {
        self.key.object.is_some()
    }

    pub(crate) fn get_meta(&self) -> ComponentBuffer {
        self.vtable.meta.get(*self)
    }

    pub(crate) fn meta_ref(&self) -> &ComponentBuffer {
        self.vtable.meta.get_ref(*self)
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
