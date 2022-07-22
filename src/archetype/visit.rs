use super::{Slot, StorageBorrowDyn};

#[non_exhaustive]
pub struct VisitData<'a> {
    pub data: &'a StorageBorrowDyn<'a>,
    pub slot: Slot,
}

impl<'a> VisitData<'a> {
    pub fn new(data: &'a StorageBorrowDyn<'a>, slot: Slot) -> Self {
        Self { data, slot }
    }
}

/// A visitor is a kind of component which is added to another component to
/// extend its functionality. Common usages such as Cloning, Serializing or
/// Debugging.
///
/// The concrete visitor is given by the component added to the visited
/// component.
///
/// **Note**: This is a low level API.
pub trait Visitor<'a> {
    /// The artefact after visiting a value
    type Visited;
    unsafe fn visit(&'a self, visit: VisitData<'a>) -> Self::Visited;
}
