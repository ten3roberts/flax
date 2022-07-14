use std::{fmt::Write, slice};

use crate::{Component, ComponentValue};

use super::ComponentInfo;

#[derive(PartialEq)]
pub struct Visit {
    pub len: usize,
    pub data: *const u8,
    pub component: ComponentInfo,
}

/// A visitor is a kind of component which is added to another component to
/// extend its functionality. Common usages such as Cloning, Serializing or
/// Debugging.
///
/// The concrete visitor is given by the component added to the visited
/// component.
///
/// **Note**: This is a low level API.
pub trait Visitor<Ctx> {
    unsafe fn visit(&mut self, ctx: &mut Ctx, visit: Visit);
}

pub struct DebugVisitor {
    func: unsafe fn(&mut dyn Write, Visit),
}

impl DebugVisitor {
    pub fn new<T>(_: Component<T>) -> Self
    where
        T: ComponentValue + std::fmt::Debug,
    {
        Self {
            func: |f, visit| unsafe {
                let val = slice::from_raw_parts(visit.data.cast::<T>(), visit.len);
                write!(f, "{}: {:#?}\n", visit.component.name(), val).expect("Failed to write");
            },
        }
    }
}

impl<W> Visitor<W> for DebugVisitor
where
    W: Write,
{
    unsafe fn visit(&mut self, ctx: &mut W, visit: Visit) {
        (self.func)(ctx, visit)
    }
}
