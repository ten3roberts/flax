//! Flax is a performant and easy to use Entity Component System.
//!
//! The world is organized by simple identifiers known as an `Entity`, which can have any number of components attached to them.
//!
//! Systems operate and iterate upon entities and their attached components and
//! provide the application logic.
//!
//! ## Features
//! - Queries
//! - Change detection
//! - Query filtering
//! - System execution
//! - Multithreaded system execution through `Schedule`
//! - Builtin many to many entity relation and graphs
//! - Reflection through component metadata
//! - Ergonomic entity builder
//! - Tracing
//! - Serialization and deserialization
//! - Runtime components
//!
//! ## Consider reading the **[User Guide](https://ten3roberts.github.io/flax/)**
//!
//! For those of you who prefer a more complete example, consider checking out an asteroid game
//! made by flax and macroquad [here](./asteroids/src/main.rs)
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
//!
//! # fn main() -> color_eyre::Result<()> {
//!
//! # let mut world = World::new();
//!
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
//! See [the guide](https://ten3roberts.github.io/flax/fundamentals/relations.html) for more
//! details.
//!
//! ```rust
//! # use flax::*;
//! component! {
//!     child_of(parent): () => [Debug],
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
//! This can lead to situations such as this:
//!
//! ```rust,ignore
//! struct Position(Vec3);
//! struct Velocity(Vec3);
//!
//! let vel = world.get::<Velocity>(entity);
//! let mut pos = world.get_mut::<Position>(entity);
//! let dt = 0.1;
//!
//! *pos = Position(**pos + **vel * dt);
//! ```
//!
//! Which in Flax is:
//!
//! ```rust
//! # use flax::*;
//! # use glam::*;
//! component! {
//!     velocity: Vec3,
//!     position: Vec3,
//! }
//! # fn main() -> color_eyre::Result<()> {
//! # let mut world = World::new();
//! # let entity = EntityBuilder::new().set(velocity(), glam::vec3(1.0, 3.0, 5.0)).set_default(position()).spawn(&mut world);
//!
//! let vel = world.get(entity, velocity())?;
//! let mut pos = world.get_mut(entity, position())?;
//! let dt = 0.1;
//!
//! *pos = *pos + *vel * dt;
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
//! library, and the author [Ralith](https://github.com/Ralith) has been awesome in bringing some pull
//! requests in.
//!
//! Despite this, I often made subtle bugs with *similar* types. The game engine was
//! cluttered with gigantic newtypes for `Velocity`, `Position` with many deref
//! coercions in order to coexist.
//!
//! ## Unsafe
//! This library makes use of unsafe for type erasure and the allocation in storage
//! of ComponentBuffers and Archetypes.

#![deny(missing_docs)]
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
mod query;
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
mod meta;
/// System execution
pub mod schedule;
/// Provides tuple utilities like `cloned`
mod util;
/// Provides a debug visitor
pub mod visitors;

/// Allows for efficient serialization and deserialization of the world and the
/// entities therein
#[cfg(feature = "serde")]
pub mod serialize;

// Required due to macro
pub(crate) use archetype::*;
pub use archetype::{ArchetypeId, BatchSpawn, ChangeKind, ComponentInfo};
pub use commands::CommandBuffer;
pub use component::*;
pub use components::*;
pub use entity::{entity_ids, Entity, EntityBuilder};
pub use entity_ref::*;
pub use error::Error;
pub use fetch::{
    relations_like, EntityIds, Fetch, FetchExt, FetchItem, Mutable, Opt, OptOr, Relations,
};
pub use filter::{All, And, CmpExt, Filter, Or, StaticFilter, With, Without};
pub use meta::*;
pub use paste::paste;
pub use query::*;
pub use schedule::*;
pub use system::*;
pub use visitors::*;
pub use world::*;

pub use flax_derive::*;
