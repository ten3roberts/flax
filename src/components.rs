//! This module contains standard components that different libraries can agree
//! on, though they don't have to.

use crate::component;

use crate::ComponentInfo;
use crate::Debug;

component! {
    /// A name for an entity of component
    pub name: String => [ Debug ],
    /// The default parent relationship
    pub child_of: (),
    /// Tag for anything which is a component.
    /// Added automatically to all components.
    /// This is the basis of the reflection provided by flax
    pub component: ComponentInfo,
}
