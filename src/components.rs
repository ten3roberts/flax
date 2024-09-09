//! This module contains standard components that different libraries can agree
//! on, though they don't have to.

use alloc::string::String;

use crate::component;
use crate::Exclusive;

use crate::component::ComponentDesc;
use crate::Debuggable;

component! {
    /// An opinionated name component, so that different libraries can agree on a "name" or "label"
    /// kind of component.
    ///
    /// This name will be used in *Display* and *Debug* impls of entities to make them more readable, as opposed to just the id.
    pub name: String => [ Debuggable ],
    /// Exclusive parent-child relation ship.
    ///
    /// Only one parent can exist for an entity. Adding a second relationship will override the
    /// existing one, effectively moving the subtree.
    pub child_of(parent): () => [ Debuggable, Exclusive ],

    /// Contains type erased metadata.
    ///
    /// Added automatically to all components.
    /// This is the basis of the reflection provided by flax
    pub component_info: ComponentDesc => [ Debuggable ],

    /// Added automatically to all STATIC entities
    pub is_static_entity: () => [ Debuggable ],
}
