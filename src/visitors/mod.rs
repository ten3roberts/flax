use std::{fmt::Write, slice};

use crate::{
    archetype::{VisitData, Visitor},
    component, ComponentValue,
};

/// Format a component with debug
pub struct DebugVisitor {
    func: unsafe fn(&mut dyn Write, VisitData),
}

impl DebugVisitor {
    pub fn new<T>() -> Self
    where
        T: ComponentValue + std::fmt::Debug,
    {
        Self {
            func: |f, visit| unsafe {
                let val = slice::from_raw_parts(visit.data.cast::<T>(), visit.len);
                // if !val.is_empty() {
                writeln!(f, "{}: {:#?}", visit.component.name(), val).expect("Failed to write");
                // }
            },
        }
    }
}

impl<W> Visitor<W> for DebugVisitor
where
    W: Write,
{
    unsafe fn visit(&mut self, ctx: &mut W, visit: VisitData) {
        (self.func)(ctx, visit)
    }
}

component! {
    pub debug_visitor: DebugVisitor,
}
