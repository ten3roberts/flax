use crate::components::name;
use crate::{ComponentBuffer, ComponentInfo, ComponentValue};

/// Additional data that can attach itself to a component
///
/// Implementors of this trait are proxy types for attaching the proper
/// components.
pub trait MetaData<T: ComponentValue> {
    /// Attach the metadata to the component buffer
    fn attach(component: ComponentInfo, buffer: &mut ComponentBuffer);
}

#[derive(Debug, Clone)]
/// Name metadata
pub struct Name;

impl<T> MetaData<T> for Name
where
    T: ComponentValue,
{
    fn attach(component: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(name(), component.name().to_string());
    }
}

#[cfg(test)]
mod test {
    use crate::{component, debug_visitor};

    use super::*;

    #[test]
    fn metadata_attach() {
        component! {
            foo: String => [crate::Debug],
        }

        let meta = foo().get_meta();

        assert!(meta.get(debug_visitor()).is_some());
        assert_eq!(meta.get(name()), Some(&"foo".to_string()));
    }
}
