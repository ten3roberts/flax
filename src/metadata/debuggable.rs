use core::{any::Any, fmt::Debug};

use crate::{
    archetype::{Slot, Storage},
    buffer::ComponentBuffer,
    component, ComponentInfo, ComponentValue,
};

use super::Metadata;

component! {
    /// Allows visiting and debug formatting the component
    pub debuggable: Debuggable,
}

#[derive(Clone)]
/// Formats a component value using [`Debug`](core::fmt::Debug)
pub struct Debuggable {
    pub(crate) debug_any: fn(&dyn Any) -> &dyn Debug,
    pub(crate) debug_ptr: fn(&*const u8) -> &dyn Debug,
    pub(crate) debug_storage: fn(&Storage, slot: Slot) -> &dyn Debug,
}

impl Debuggable {
    /// Formats the given value
    pub fn debug<'a>(&self, value: &'a dyn Any) -> &'a dyn Debug {
        (self.debug_any)(value)
    }

    pub(crate) unsafe fn debug_ptr<'a>(&self, ptr: &'a *const u8) -> &'a dyn Debug {
        (self.debug_ptr)(ptr)
    }
}

impl<T> Metadata<T> for Debuggable
where
    T: Sized + core::fmt::Debug + ComponentValue,
{
    fn attach(_: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(
            debuggable(),
            Debuggable {
                debug_any: |value| value.downcast_ref::<T>().unwrap(),
                debug_storage: |storage, slot| &storage.downcast_ref::<T>()[slot],
                debug_ptr: |value| unsafe { &*(value.cast::<T>()) },
            },
        );
    }
}
