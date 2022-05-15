//! A fast yet simple to use entity component system (ECS)
//!
//! Components are declared by their identifier and a type, which allows the
//! same type to be used for multiple distinct components.
//!
//! This removes the need for newtype and dereferencingy and the many of derives
//! present in other ECS implementations.
//!
//! # Features
//! - Easy random access
//! - Declarative queries and iteration
//! - Change detection

mod archetype;
mod component;
mod entity;
#[macro_use]
pub mod macros;
mod buffer;
mod util;
mod world;

pub use paste::paste;

pub use buffer::*;
pub use component::*;
pub use entity::Entity;
pub use world::*;
