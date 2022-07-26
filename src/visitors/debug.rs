use core::fmt;
use std::{
    collections::BTreeMap,
    fmt::{Debug, Formatter},
};

use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Slot, VisitData, Visitor},
    component, Archetype, ComponentId, ComponentValue,
};

/// Format a component with debug
pub struct DebugVisitor {
    visit: unsafe fn(VisitData<'_>) -> &'_ dyn Debug,
}

impl DebugVisitor {
    pub fn new<T>() -> Self
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
    type Visited = &'a dyn Debug;

    unsafe fn visit(&'a self, visit: VisitData<'a>) -> Self::Visited {
        (self.visit)(visit)
    }
}

component! {
    pub debug_visitor: DebugVisitor,
}

pub struct RowFormatter<'a> {
    pub arch: &'a Archetype,
    pub slot: Slot,
    pub meta:
        &'a BTreeMap<ComponentId, (Option<AtomicRef<'a, DebugVisitor>>, AtomicRef<'a, String>)>,
}

struct MissingDebug;

impl Debug for MissingDebug {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "_")
    }
}

impl<'a> RowFormatter<'a> {
    pub fn new(
        arch: &'a Archetype,
        slot: Slot,
        meta: &'a BTreeMap<
            ComponentId,
            (Option<AtomicRef<'a, DebugVisitor>>, AtomicRef<'a, String>),
        >,
    ) -> Self {
        Self { arch, slot, meta }
    }
}

impl<'a> Debug for RowFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for storage in self.arch.storages() {
            if let Some((visitor, name)) = self.meta.get(&storage.info().id()) {
                if let Some(visitor) = visitor {
                    unsafe {
                        let data = VisitData::new(&storage, self.slot);

                        map.entry(name, visitor.visit(data));
                    }
                } else {
                    map.entry(name, &MissingDebug);
                };
            } else {
                map.entry(&storage.info().name(), &MissingDebug);
            }
        }

        map.finish()
    }
}
