use core::fmt::{self, Formatter};

use crate::{
    archetype::{Slot, Storage},
    buffer::ComponentBuffer,
    component, Archetype, ComponentInfo, ComponentKey, ComponentValue, MetaData, World,
};

/// Format a component with debug
pub struct DebugVisitor {
    visit: for<'x> unsafe fn(&'x Storage, slot: Slot) -> &'x dyn fmt::Debug,
}

impl DebugVisitor {
    /// Creates a new debug visitor visiting values of type `T`
    fn new<T>() -> Self
    where
        T: ComponentValue + core::fmt::Debug,
    {
        Self {
            visit: |storage, slot| unsafe { storage.get::<T>(slot).unwrap() },
        }
    }
}

component! {
    /// Allows visiting and debug formatting the component
    pub debug_visitor: DebugVisitor,
}

#[derive(Debug, Clone)]
/// Forward the debug implementation to the component
pub struct Debug;

impl<T> MetaData<T> for Debug
where
    T: core::fmt::Debug + ComponentValue,
{
    fn attach(_: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(debug_visitor(), DebugVisitor::new::<T>());
    }
}

struct MissingDebug;

impl fmt::Debug for MissingDebug {
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

impl fmt::Debug for ComponentName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.base_name, self.id)
    }
}

impl<'a> fmt::Debug for RowValueFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for storage in self.arch.try_borrow_all().flatten() {
            let info = storage.info();

            let name = ComponentName {
                base_name: storage.info().name(),
                id: info.key(),
            };

            if let Ok(visitor) = self.world.get(info.key().id, debug_visitor()) {
                unsafe {
                    map.entry(&name, (visitor.visit)(&storage, self.slot));
                }
            } else {
                map.entry(&name, &MissingDebug);
            }
        }

        map.finish()
    }
}
