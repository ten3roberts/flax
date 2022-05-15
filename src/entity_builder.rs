use crate::{Component, ComponentBuffer, ComponentValue};

pub struct EntityBuilder {
    components: ComponentBuffer,
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self {
            components: ComponentBuffer::new(),
        }
    }

    pub fn with<T: ComponentValue>(mut self, component: Component<T>, value: T) -> Self {
        self.components.insert(component, value);
        self
    }
}

impl Default for EntityBuilder {
    fn default() -> Self {
        Self::new()
    }
}
