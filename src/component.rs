use std::{
    marker::PhantomData,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};

pub trait ComponentValue: Send + Sync + 'static {}

impl<T> ComponentValue for T where T: Send + Sync + 'static {}

static MAX_ID: AtomicU64 = AtomicU64::new(5);
// A value of 0 means the typeid has yet to be aquired
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComponentId(u64);

impl ComponentId {
    pub fn as_raw(self) -> u64 {
        self.0
    }
}

impl ComponentId {
    /// Return a unique always incrementing id for each invocation
    pub(crate) fn unique_id() -> u64 {
        MAX_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    pub fn static_init(id: &AtomicU64) -> ComponentId {
        let v = id.load(Relaxed);
        if v == 0 {
            let v = ComponentId::unique_id();
            id.store(v, Relaxed);
            ComponentId(v)
        } else {
            ComponentId(v)
        }
    }
}

/// Defines a strongly typed component
pub struct Component<T> {
    id: ComponentId,
    marker: PhantomData<T>,
}

impl<T> Eq for Component<T> {}

impl<T> PartialEq for Component<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Copy for Component<T> {}

impl<T> Clone for Component<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            marker: PhantomData,
        }
    }
}

impl<T: ComponentValue> std::fmt::Debug for Component<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Component").field("id", &self.id).finish()
    }
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
    #[inline(always)]
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
