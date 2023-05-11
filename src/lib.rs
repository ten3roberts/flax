//! Flax is a performant and easy to use Entity Component System.
//!
//! The world is organized by simple identifiers known as an [Entity](https://docs.rs/flax/latest/flax/entity/struct.Entity.html), which can have any number of components attached to them.
//!
//! Systems operate on the world's entities and provide the application logic.
//!
//! Consider reading the [**User Guide**](https://ten3roberts.github.io/flax/guide)
//!
//! ## Features
//! - [Declarative component macro](https://docs.rs/flax/latest/flax/macro.component.html)
//! - [Queries](https://docs.rs/flax/latest/flax/struct.Query.html)
//! - [Change detection](https://docs.rs/flax/latest/flax/struct.Component.html#method.modified)
//! - [Query filtering](https://docs.rs/flax/latest/flax/filter/index.html)
//! - [System execution](https://docs.rs/flax/latest/flax/system/struct.System.html)
//! - [Multithreaded system execution through](https://docs.rs/flax/latest/flax/schedule/struct.Schedule.html)
//! - [Many to many entity relation and graphs](https://docs.rs/flax/latest/flax/macro.component.html#relations)
//! - [Reflection through component metadata](https://docs.rs/flax/latest/flax/macro.component.html)
//! - [Ergonomic entity builder](https://docs.rs/flax/latest/flax/struct.EntityBuilder.html)
//! - [Serialization and deserialization](https://docs.rs/flax/latest/flax/serialize/)
//! - [(async) event subscription](https://docs.rs/flax/latest/flax/struct.World.html#method.subscribe)
//! - [Runtime components](https://docs.rs/flax/latest/flax/struct.World.html#method.spawn_component)
//!
//! ## [Live Demo](https://ten3roberts.github.io/flax/asteroids)
//! See a live demo of asteroids using wasm [here](https://ten3roberts.github.io/flax/asteroids).
//!
//! ## Example Usage
//!
//! ```rust
//!   // Declare static components
//!   use flax::*;
//!   component! {
//!     health: f32,
//!     regen: f32,
//!     pos: (f32, f32),
//!     player: (),
//!     items: Vec<String>,
//!   }
//!
//!   let mut world = World::new();
//!
//!   // Spawn an entity
//!   let p = EntityBuilder::new()
//!       .set(health(), 50.0)
//!       .tag(player())
//!       .set(pos(), (0.0, 0.0))
//!       .set(regen(), 1.0)
//!       .set_default(items())
//!       .spawn(&mut world);
//!
//!   let mut query = Query::new((health().as_mut(), regen()));
//!
//!   // Apply health regen for all match entites
//!   for (health, regen) in &mut query.borrow(&world) {
//!       *health = (*health + regen).min(100.0);
//!   }
//!
//! ```
//!
//! ## Systems
//! Queries with logic can be abstracted into a system, and multiple systems can be
//! collected into a schedule.
//!
//! ```rust
//! # use flax::*;
//! # component! {
//! #   health: f32,
//! #   regen: f32,
//! #   pos: (f32, f32),
//! #   player: (),
//! #   items: Vec<String>,
//! # }
//! # fn main() -> anyhow::Result<()> {
//! # let mut world = World::new();
//! let regen_system = System::builder()
//!     .with(Query::new((health().as_mut(), regen())))
//!     .for_each(|(health, regen)| {
//!         *health = (*health + regen).min(100.0);
//!     })
//!     .boxed();
//!
//! let despawn_system = System::builder()
//!     .with(Query::new(entity_ids()).filter(health().le(0.0)))
//!     .write::<CommandBuffer>()
//!     .build(|mut q: QueryBorrow<EntityIds, _>, cmd: &mut CommandBuffer| {
//!         for id in &mut q {
//!             cmd.despawn(id);
//!         }
//!     })
//!     .boxed();
//!
//! let mut schedule = Schedule::from([regen_system, despawn_system]);
//!
//! schedule.execute_par(&mut world)?;
//!
//! # Ok(())
//! # }
//! ```
//! ## Relations
//!
//! Flax provides first class many-many relations between entities, which is useful for tree scene
//! hierarchies, graphs, and physics joints between entities.
//!
//! Relations can be both state-less or have associated data, like spring or joint strengths.
//!
//! Relations are cache friendly and querying children of does not require random access. In
//! addition, relations are cleaned up on despawns and are stable during serialization, even if the
//! entity ids migrate due to collisions.
//!
//! See [the guide](https://ten3roberts.github.io/flax/guide/fundamentals/relations.html) for more
//! details.
//!
//! ```rust
//! # use flax::*;
//! component! {
//!     child_of(parent): () => [ Debuggable ],
//! }
//!
//! let mut world = World::new();
//!
//! let parent = Entity::builder()
//!     .set(name(), "Parent".into())
//!     .spawn(&mut world);
//!
//! let child1 = Entity::builder()
//!     .set(name(), "Child1".into())
//!     .set_default(child_of(parent))
//!     .spawn(&mut world);
//!
//!
//! ```
//!
//!
//! ## Comparison to other ECS
//!
//! Compared to other ecs implementations, a component is simply another `Entity`
//! identifier to which data is attached. This means the same "type" can be added to
//! an entity multiple times.
//!
//! A limitation of existing implementations such as [specs](https://github.com/amethyst/specs), [planck](https://github.com/jojolepro/planck_ecs/), or [hecs](https://github.com/Ralith/hecs) is that newtype wrappers need to be created to allow components of the same inner type to coexist.
//!
//! This leads to having to forward all trait implementations trough e.g
//! `derive-more` or dereferencing the newtypes during usage.
//!
//! By making components separate from the type the components can work together without deref or
//! newtype construction.
//!
//! ```rust
//! # use flax::*;
//! # use glam::*;
//! component! {
//!     velocity: Vec3,
//!     position: Vec3,
//! }
//! # fn main() -> anyhow::Result<()> {
//! # let mut world = World::new();
//! # let entity = EntityBuilder::new().set(velocity(), glam::vec3(1.0, 3.0, 5.0)).set_default(position()).spawn(&mut world);
//!
//! let vel = world.get(entity, velocity())?;
//! let mut pos = world.get_mut(entity, position())?;
//! let dt = 0.1;
//!
//! *pos += *vel * dt;
//! # Ok(())
//! # }
//! ```
//!
//! On a further note, since the components have to be declared beforehand (not
//! always true, more on that later), it limits the amount of types which can be
//! inserted as components. This fixes subtle bugs which come by having the type
//! dictate the component, such as inserting an `Arc<Type>` instead of just `Type`,
//! which leads to subsequent systems not finding the `Type` on the entity.
//!
//! Having statically declared componenents makes the rust type system disallow
//! these cases and catches these bugs earlier.
//!
//! ## Motivation
//!
//! During development of a game in school I used the `hecs` ECS. It is an awesome
//! library, and the author [Ralith](https://github.com/Ralith) has been wonderful in accepting
//! contributions and inquiries.
//!
//! Despite this, I often made subtle bugs with *similar* types. The game engine was
//! cluttered with gigantic newtypes for `Velocity`, `Position` with many deref
//! coercions in order to coexist.
//!
//! ## Unsafe
//! This library makes use of unsafe for type erasure and the allocation in storage
//! of ComponentBuffers and Archetypes.

#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

/// Structured component storage
pub mod archetype;
/// Provides a buffer for holding multiple types simultaneously
pub mod buffer;
/// Contains a commandbuffer
pub mod commands;
mod component;
/// Provides entity identifiers
pub mod entity;
/// Filter items yielded queries
pub mod filter;
mod system;
mod world;

pub mod components;
mod entity_ref;
mod entry;
/// Defines the single error type and result alias
pub mod error;
/// Traits for fetching multiple component values simultaneously
pub mod fetch;
#[macro_use]
mod macros;
/// Provides a debug visitor
// mod cascade;
mod archetypes;
/// Subscribe to changes in the world
pub mod events;
mod metadata;
/// Query the world
pub mod query;
mod relation;
/// System execution
pub mod schedule;
/// Allows for efficient serialization and deserialization of the world and the
/// entities therein
#[cfg(feature = "serde")]
pub mod serialize;
/// Provides tuple utilities like `cloned`
mod util;
/// vtable implementation for dynamic dispatching
pub mod vtable;

// Required due to macro
pub(crate) use archetype::*;
pub use archetype::{ArchetypeId, BatchSpawn};
pub use commands::CommandBuffer;
pub use component::*;
pub use components::*;
pub use entity::{entity_ids, Entity, EntityBuilder};
pub use entity_ref::*;
pub use entry::{Entry, OccupiedEntry, VacantEntry};
pub use error::Error;
pub use fetch::{
    relations_like, EntityIds, Fetch, FetchExt, FetchItem, Mutable, Opt, OptOr, Relations,
};
pub use filter::{All, And, Cmp, Nothing, Or, With, Without};
pub use metadata::*;
pub(crate) use query::ArchetypeSearcher;
pub use query::*;
pub use relation::*;
pub use schedule::*;
pub use system::*;
pub(crate) use vtable::*;
pub use world::*;

#[cfg(feature = "derive")]
pub use flax_derive::*;
