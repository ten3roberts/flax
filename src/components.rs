//! This module contains standard components that different libraries can agree
//! on, though they don't have to.

use alloc::string::String;

use crate::component;

use crate::ComponentInfo;
use crate::Debuggable;

component! {
    /// A name for an entity of component
    pub name: String => [ Debuggable ],
    /// The default parent relationship
    pub child_of(parent): () => [ Debuggable ],

    /// Contains type erased metadata.
    ///
    /// Added automatically to all components.
    /// This is the basis of the reflection provided by flax
    pub component_info: ComponentInfo => [ Debuggable ],

    /// Added automatically to all STATIC entities
    pub is_static: () => [ Debuggable ],
}
