use std::{fmt::Write, slice};

use crate::{Component, ComponentValue};

use super::ComponentInfo;

#[derive(PartialEq)]
/// The current component being visited.
pub struct VisitData {
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
    unsafe fn visit(&mut self, ctx: &mut Ctx, visit: VisitData);
}
