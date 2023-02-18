use core::{alloc::Layout, any::TypeId, marker::PhantomData, mem};

use crate::{buffer::ComponentBuffer, ComponentInfo, ComponentValue};

#[derive(PartialEq, Eq)]
/// Describes a components dynamic functionality, such as name, metadata, and type layout.
pub struct UntypedVTable {
    pub(crate) name: &'static str,
    pub(crate) drop: unsafe fn(*mut u8),
    pub(crate) layout: Layout,
    pub(crate) type_id: fn() -> TypeId,
    pub(crate) type_name: fn() -> &'static str,
    /// A metadata is a component which is attached to the component, such as
    /// metadata or name
    pub(crate) meta: fn(ComponentInfo) -> ComponentBuffer,
}

impl UntypedVTable {
    /// Returns true if the vtable is of type `T`
    pub fn is<T: ComponentValue>(&self) -> bool {
        (self.type_id)() == TypeId::of::<T>()
    }

    /// Creates a new vtable of type `T`
    pub(crate) const fn new<T: ComponentValue>(
        name: &'static str,
        meta: fn(ComponentInfo) -> ComponentBuffer,
    ) -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }

        UntypedVTable {
            name,
            meta,
            drop: drop_ptr::<T>,
            layout: Layout::new::<T>(),
            type_id: || TypeId::of::<T>(),
            type_name: || core::any::type_name::<T>(),
        }
    }
}

/// Represents a strongly typed vtable
#[repr(transparent)]
pub struct ComponentVTable<T> {
    inner: UntypedVTable,
    marker: PhantomData<T>,
}

impl<T> core::ops::Deref for ComponentVTable<T> {
    type Target = UntypedVTable;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ComponentValue + Eq> Eq for ComponentVTable<T> {}

impl<T: ComponentValue + PartialEq> PartialEq for ComponentVTable<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner && self.marker == other.marker
    }
}

impl<T: ComponentValue> ComponentVTable<T> {
    pub(crate) fn from_untyped(vtable: &'static UntypedVTable) -> &'static Self {
        if !vtable.is::<T>() {
            panic!("Mismathed type");
        }

        unsafe { mem::transmute(vtable) }
    }

    /// Creates a new *typed* vtable of `T`
    pub const fn new(name: &'static str, meta: fn(ComponentInfo) -> ComponentBuffer) -> Self {
        Self {
            inner: UntypedVTable::new::<T>(name, meta),
            marker: PhantomData,
        }
    }

    pub(crate) fn erase(&self) -> &'static UntypedVTable {
        unsafe { mem::transmute(self) }
    }
}
