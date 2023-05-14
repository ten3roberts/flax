use core::{
    any::Any,
    fmt::{self, Debug, Formatter},
};

use crate::{
    archetype::{Slot, Storage},
    buffer::ComponentBuffer,
    component, Archetype, ComponentInfo, ComponentKey, ComponentValue, Metadata, World,
};

component! {
    /// Allows visiting and debug formatting the component
    pub debuggable: Debuggable,
}

#[derive(Clone)]
/// Formats a component value using [`Debug`](core::fmt::Debug)
pub struct Debuggable {
    debug: fn(&dyn Any) -> &dyn Debug,
    debug_ptr: fn(&*const u8) -> &dyn Debug,
    debug_storage: fn(&Storage, slot: Slot) -> &dyn Debug,
}

impl Debuggable {
    /// Formats the given value
    pub fn debug<'a>(&self, value: &'a dyn Any) -> &'a dyn Debug {
        (self.debug)(value)
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
                debug: |value| value.downcast_ref::<T>().unwrap(),
                debug_storage: |storage, slot| &storage.downcast_ref::<T>()[slot],
                debug_ptr: |value| unsafe { &*(value.cast::<T>()) },
            },
        );
    }
}

pub(crate) struct MissingDebug;

impl Debug for MissingDebug {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "...")
    }
}

pub(crate) struct RowValueFormatter<'a> {
    pub world: &'a World,
    pub arch: &'a Archetype,
    pub slot: Slot,
}

struct ComponentName {
    base_name: &'static str,
    id: ComponentKey,
}

impl Debug for ComponentName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.base_name, self.id)
    }
}

impl<'a> Debug for RowValueFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for storage in self.arch.try_borrow_all().flatten() {
            let info = storage.info();

            if let Ok(visitor) = self.world.get(info.key().id, debuggable()) {
                map.entry(&info, (visitor.debug_storage)(&storage, self.slot));
            } else {
                map.entry(&info, &MissingDebug);
            }
        }

        map.finish()
    }
}
