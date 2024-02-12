use crate::{
    buffer::ComponentBuffer,
    component::{ComponentDesc, ComponentValue},
    components::name,
};

mod debuggable;
mod relation;

pub use debuggable::*;
pub use relation::*;

/// Additional data that can attach itself to a component
///
/// Implementors of this trait are proxy types for attaching the proper
/// components.
pub trait Metadata<T: ComponentValue> {
    /// Attach the metadata to the component buffer
    fn attach(desc: ComponentDesc, buffer: &mut ComponentBuffer);
}

#[derive(Debug, Clone)]
/// Provides a name for components
pub struct Name;

impl<T> Metadata<T> for Name
where
    T: ComponentValue,
{
    fn attach(desc: ComponentDesc, buffer: &mut ComponentBuffer) {
        buffer.set(name(), desc.name().into());
    }
}

#[cfg(test)]
mod test {
    use alloc::string::String;

    use crate::component;

    use super::*;

    #[test]
    fn metadata_attach() {
        component! {
            foo: String => [crate::Debuggable],
        }

        let meta = foo().desc().create_meta();

        assert!(meta.get(debuggable()).is_some());
        assert_eq!(meta.get(name()), Some(&"foo".into()));
    }
}
