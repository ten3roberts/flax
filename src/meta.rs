use crate::components::name;
use crate::visitors::debug_visitor;
use crate::{visitors::DebugVisitor, Component, ComponentBuffer, ComponentInfo, ComponentValue};

/// Additional data that can attach itself to a component
///
/// Implementors of this trait are proxy types for attaching the proper
/// components.
pub trait MetaData<T: ComponentValue> {
    /// Attach the metadata to the component buffer
    fn attach(component: ComponentInfo, buffer: &mut ComponentBuffer);
}

#[derive(Debug, Clone)]
pub struct Name;

impl<T> MetaData<T> for Name
where
    T: ComponentValue,
{
    fn attach(component: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(name(), component.name().to_string());
    }
}

#[derive(Debug, Clone)]
/// Forward the debug implementation to the component
pub struct Debug;

impl<T> MetaData<T> for Debug
where
    T: std::fmt::Debug + ComponentValue,
{
    fn attach(component: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(debug_visitor(), DebugVisitor::new::<T>());
    }
}

#[cfg(test)]
mod test {
    use crate::component;

    use super::*;

    #[test]
    fn metadata_attach() {
        component! {
            foo: String => [Debug],
        }

        let meta = foo().get_meta();

        assert!(meta.get(debug_visitor()).is_some());
        assert_eq!(meta.get(name()), Some(&"foo".to_string()));
    }
}
