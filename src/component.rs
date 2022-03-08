use std::{any::TypeId, marker::PhantomData, num::NonZeroU64, ptr::NonNull};

pub trait Component: Send + Sync + 'static {}

impl<T> Component for T where T: Send + Sync + 'static {}

pub type ComponentId = TypeId;

/// Defines a strongly typed component
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ComponentDef<T: Component> {
    id: ComponentId,
    marker: PhantomData<T>,
}

impl<T: Component> ComponentDef<T> {
    pub fn typed<V: 'static>(id: ComponentId) -> Self {
        Self {
            id: TypeId::of::<V>(),
            marker: PhantomData,
        }
    }
}

macro_rules! new_component {
    ($name: ident => $ty: ident) => {
        pub fn $name() -> ComponentDef<$ty> {
            ComponentDef::typed::<$name>()
        }
    };
}
