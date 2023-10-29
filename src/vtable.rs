use core::{alloc::Layout, any::TypeId, marker::PhantomData, mem, ptr::NonNull};

use once_cell::sync::OnceCell;

use crate::{
    buffer::ComponentBuffer,
    component::{ComponentDesc, ComponentValue},
};

#[doc(hidden)]
pub struct LazyComponentBuffer {
    value: OnceCell<ComponentBuffer>,
    init: fn(ComponentDesc) -> ComponentBuffer,
}

impl LazyComponentBuffer {
    /// Creates a new component buffer which can also be recreated
    pub const fn new(init: fn(ComponentDesc) -> ComponentBuffer) -> Self {
        Self {
            value: OnceCell::new(),
            init,
        }
    }

    pub(crate) fn get_ref(&self, desc: ComponentDesc) -> &ComponentBuffer {
        self.value.get_or_init(|| (self.init)(desc))
    }

    pub(crate) fn get(&self, desc: ComponentDesc) -> ComponentBuffer {
        (self.init)(desc)
    }
}

/// Describes a components dynamic functionality, such as name, metadata, and type layout.
pub struct UntypedVTable {
    pub(crate) name: &'static str,
    pub(crate) drop: unsafe fn(*mut u8),
    pub(crate) layout: Layout,
    pub(crate) type_id: fn() -> TypeId,
    pub(crate) type_name: fn() -> &'static str,
    // Dangling pointer with proper alignment
    // See: https://github.com/rust-lang/rust/issues/55724
    pub(crate) dangling: fn() -> NonNull<u8>,
    /// A metadata is a component which is attached to the component, such as
    /// metadata or name
    pub(crate) meta: &'static LazyComponentBuffer,
}

impl UntypedVTable {
    /// Returns true if the vtable is of type `T`
    pub fn is<T: ComponentValue>(&self) -> bool {
        (self.type_id)() == TypeId::of::<T>()
    }

    /// Creates a new vtable of type `T`
    pub(crate) const fn new<T: ComponentValue>(
        name: &'static str,
        meta: &'static LazyComponentBuffer,
    ) -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }

        UntypedVTable {
            name,
            drop: drop_ptr::<T>,
            layout: Layout::new::<T>(),
            type_id: || TypeId::of::<T>(),
            type_name: || core::any::type_name::<T>(),
            meta,
            dangling: || NonNull::<T>::dangling().cast(),
        }
    }
}

/// Represents a strongly typed vtable
#[repr(transparent)]
pub struct ComponentVTable<T> {
    inner: UntypedVTable,
    marker: PhantomData<T>,
}

impl<T> core::fmt::Debug for ComponentVTable<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComponentVTable")
            .field("name", &self.inner.name)
            .field("type_name", &self.inner.type_name)
            .finish()
    }
}

impl<T> core::ops::Deref for ComponentVTable<T> {
    type Target = UntypedVTable;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ComponentValue> ComponentVTable<T> {
    /// Creates a new *typed* vtable of `T`
    pub const fn new(name: &'static str, meta: &'static LazyComponentBuffer) -> Self {
        Self {
            inner: UntypedVTable::new::<T>(name, meta),
            marker: PhantomData,
        }
    }

    pub(crate) fn erase(&self) -> &'static UntypedVTable {
        unsafe { mem::transmute(self) }
    }
}
