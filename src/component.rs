use std::{
    marker::PhantomData,
    sync::atomic::{
        AtomicU64,
        Ordering::{Acquire, Relaxed, Release},
    },
};

pub trait ComponentValue: Send + Sync + 'static {}

impl<T> ComponentValue for T where T: Send + Sync + 'static {}

static MAX_ID: AtomicU64 = AtomicU64::new(1);
// A value of 0 means the typeid has yet to be aquired
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentId(u64);

impl ComponentId {
    /// Return a unique always incrementing id for each invocation
    pub(crate) fn unique_id() -> u64 {
        dbg!(MAX_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }

    pub fn static_init(id: &AtomicU64) -> ComponentId {
        match id.fetch_update(Acquire, Relaxed, |v| {
            if v == 0 {
                Some(Self::unique_id())
            } else {
                None
            }
        }) {
            Ok(v) => ComponentId(v),
            Err(v) => ComponentId(v),
        }
    }
}

/// Defines a strongly typed component
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Component<T: ComponentValue> {
    id: ComponentId,
    marker: PhantomData<T>,
}

impl<T: ComponentValue> Component<T> {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id,
            marker: PhantomData,
        }
    }

    /// Get the component's id.
    #[must_use]
    pub fn id(&self) -> ComponentId {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    component! {
        foo: i32,
        bar: f32,
    }

    #[test]
    fn component_ids() {
        let c_foo = foo();
        eprintln!("Foo: {c_foo:?}");
        eprintln!("Bar: {:?}", bar().id());
        assert_ne!(foo().id(), bar().id());
        assert_eq!(foo(), foo());
    }
}
