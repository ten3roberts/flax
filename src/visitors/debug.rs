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
    debug_storage: fn(&Storage, slot: Slot) -> &dyn Debug,
}

impl Debuggable {
    /// Formats the given value
    pub fn debug<'a>(&self, value: &'a dyn Any) -> &'a dyn Debug {
        (self.debug)(value)
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
            },
        );
    }
}

struct MissingDebug;

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
        write!(f, "{}:{}", self.base_name, self.id)
    }
}

impl<'a> Debug for RowValueFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for storage in self.arch.try_borrow_all().flatten() {
            let info = storage.info();

            let name = ComponentName {
                base_name: storage.info().name(),
                id: info.key(),
            };

            if let Ok(visitor) = self.world.get(info.key().id, debuggable()) {
                map.entry(&name, (visitor.debug_storage)(&storage, self.slot));
            } else {
                map.entry(&name, &MissingDebug);
            }
        }

        map.finish()
    }
}
