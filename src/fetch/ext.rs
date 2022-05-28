use crate::{Component, ComponentValue};

pub trait FetchExt: Sized {}

impl<T> FetchExt for Component<T> where T: ComponentValue {}
