use crate::{ChangedFilter, Component, ComponentValue};

pub trait FetchExt: Sized {
    /// Transform this fetch into a change filtered fetch
    fn changed(self) -> ChangedFilter<Self>;
}

impl<T> FetchExt for Component<T>
where
    T: ComponentValue,
{
    /// Transform this fetch into a changed filter
    fn changed(self) -> ChangedFilter<Self> {
        ChangedFilter::new(self.id(), self)
    }
}
