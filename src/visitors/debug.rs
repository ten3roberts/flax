use core::fmt::{self, Formatter};

use alloc::collections::BTreeMap;
use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Slot, Storage},
    buffer::ComponentBuffer,
    component, Archetype, ComponentInfo, ComponentKey, ComponentValue, MetaData,
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
pub(crate) struct RowFormatter<'a> {
    pub arch: &'a Archetype,
    pub slot: Slot,
    pub meta: &'a BTreeMap<ComponentKey, AtomicRef<'a, DebugVisitor>>,
}

struct MissingDebug;

impl fmt::Debug for MissingDebug {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "...")
    }
}

impl<'a> RowFormatter<'a> {
    pub fn new(
        arch: &'a Archetype,
        slot: Slot,
        meta: &'a BTreeMap<ComponentKey, AtomicRef<'a, DebugVisitor>>,
    ) -> Self {
        Self { arch, slot, meta }
    }
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

impl<'a> fmt::Debug for RowFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for storage in self.arch.borrow_all() {
            let name = ComponentName {
                base_name: storage.info().name(),
                id: storage.info().key(),
            };
            if let Some(visitor) = self.meta.get(&storage.info().key()) {
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
