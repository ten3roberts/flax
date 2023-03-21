use crate::buffer::ComponentBuffer;
use crate::components::name;
use crate::{ComponentInfo, ComponentValue};

/// Additional data that can attach itself to a component
///
/// Implementors of this trait are proxy types for attaching the proper
/// components.
pub trait Metadata<T: ComponentValue> {
    /// Attach the metadata to the component buffer
    fn attach(component: ComponentInfo, buffer: &mut ComponentBuffer);
}

#[derive(Debug, Clone)]
/// Name metadata
pub struct Name;

impl<T> Metadata<T> for Name
where
    T: ComponentValue,
{
    fn attach(component: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(name(), component.name().into());
    }
}

#[cfg(test)]
mod test {
    use alloc::string::String;

    use crate::{component, debuggable};

    use super::*;

    #[test]
    fn metadata_attach() {
        component! {
            foo: String => [crate::Debuggable],
        }

        let meta = foo().get_meta();

        assert!(meta.get(debuggable()).is_some());
        assert_eq!(meta.get(name()), Some(&"foo".into()));
    }
}
