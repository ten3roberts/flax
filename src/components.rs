//! This module contains standard components that different libraries can agree
//! on, though they don't have to.

use crate::component;

component! {
    /// A name for an entity of component
    pub name: String,
    /// The default parent relationship
    pub parent: (),
}
