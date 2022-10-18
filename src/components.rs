//! This module contains standard components that different libraries can agree
//! on, though they don't have to.

use alloc::string::String;

use crate::component;

use crate::ComponentInfo;
use crate::Debug;

component! {
    /// A name for an entity of component
    pub name: String => [ Debug ],
    /// The default parent relationship
    pub child_of(parent): () => [ Debug ],
    /// Tag for anything which is a component.
    /// Added automatically to all components.
    /// This is the basis of the reflection provided by flax
    pub is_component: ComponentInfo,
    /// Added automatically to all STATIC entities
    pub is_static: (),
}
