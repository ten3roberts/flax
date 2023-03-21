use crate::buffer::ComponentBuffer;
use crate::components::name;
use crate::{ComponentInfo, ComponentValue};
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
    fn attach(info: ComponentInfo, buffer: &mut ComponentBuffer);
}

#[derive(Debug, Clone)]
/// Provides a name for components
pub struct Name;

impl<T> Metadata<T> for Name
where
    T: ComponentValue,
{
    fn attach(info: ComponentInfo, buffer: &mut ComponentBuffer) {
        buffer.set(name(), info.name().into());
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
