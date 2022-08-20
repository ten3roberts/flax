use core::fmt;
use std::{collections::BTreeMap, fmt::Formatter};

use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Slot, VisitData, Visitor},
    component, Archetype, ComponentBuffer, ComponentId, ComponentInfo, ComponentValue, MetaData,
};

/// Format a component with debug
pub struct DebugVisitor {
    visit: unsafe fn(VisitData<'_>) -> &'_ dyn fmt::Debug,
}

impl DebugVisitor {
    /// Creates a new debug visitor visiting values of type `T`
    fn new<T>() -> Self
    where
        T: ComponentValue + std::fmt::Debug,
    {
        Self {
            visit: |visit| unsafe {
                visit
                    .data
                    .at(visit.slot)
                    .expect("Out of bounds")
                    .cast::<T>()
                    .as_ref()
                    .expect("Not null")
            },
        }
    }
}

impl<'a> Visitor<'a> for DebugVisitor {
    type Visited = &'a dyn fmt::Debug;

    unsafe fn visit(&'a self, visit: VisitData<'a>) -> Self::Visited {
        (self.visit)(visit)
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
    T: std::fmt::Debug + ComponentValue,
{
    fn attach(_: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(debug_visitor(), DebugVisitor::new::<T>());
    }
}
pub(crate) struct RowFormatter<'a> {
    pub arch: &'a Archetype,
    pub slot: Slot,
    pub meta: &'a BTreeMap<ComponentId, AtomicRef<'a, DebugVisitor>>,
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
        meta: &'a BTreeMap<ComponentId, AtomicRef<'a, DebugVisitor>>,
    ) -> Self {
        Self { arch, slot, meta }
    }
}

struct ComponentName {
    base_name: &'static str,
    id: ComponentId,
}

impl fmt::Debug for ComponentName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.id.is_relation() {
            write!(f, "{}({})", self.base_name, self.id.high())
        } else {
            write!(f, "{}", self.base_name)
        }
    }
}

impl<'a> fmt::Debug for RowFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for storage in self.arch.borrow_all() {
            let name = ComponentName {
                base_name: storage.info().name(),
                id: storage.info().id(),
            };
            if let Some(visitor) = self.meta.get(&storage.info().id()) {
                unsafe {
                    let data = VisitData::new(&storage, self.slot);

                    map.entry(&name, visitor.visit(data));
                }
            } else {
                map.entry(&name, &MissingDebug);
            }
        }

        map.finish()
    }
}
