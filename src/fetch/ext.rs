use crate::{Component, ComponentValue, Mutable};

pub trait FetchExt: Sized {}

impl<T> FetchExt for Component<T> {}
