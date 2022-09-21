[ ![GitHub Workflow Status](https://img.shields.io/github/workflow/status/ten3roberts/flax/main?style=flat) ](https://github.com/ten3roberts/flax/actions)
[ ![Crates](https://img.shields.io/crates/v/flax?style=flat) ](https://crates.io/crates/flax)
[ ![Docs](https://img.shields.io/docsrs/flax?style=flat) ](https://docs.rs/flax)
[ ![Codecov](https://img.shields.io/codecov/c/github/ten3roberts/flax?style=flat) ](https://app.codecov.io/gh/ten3roberts/flax)
[ ![Guide](https://img.shields.io/badge/guide-Read%20the%20guide-blue) ](https://ten3roberts.github.io/flax)

# Flax

<!-- cargo-sync-readme start -->

Flax is a performant and easy to use Entity Component System.

The world is organized by simple identifiers known as an `Entity`, which can have any number of components attached to them.

Systems operate and iterate upon entities and their attached components and
provide the application logic.

## Features
- Queries
- Change detection
- Query filtering
- System execution
- Multithreaded system execution through `Schedule`
- Builtin many to many entity relation and graphs
- Reflection through component metadata
- Ergonomic entity builder
- Tracing
- Serialization and deserialization
- Runtime components

## Consider reading the **[User Guide](https://ten3roberts.github.io/flax/)**

For those of you who prefer a more complete example, consider checking out an asteroid game
made by flax and macroquad [here](./asteroids/src/main.rs)

## Example Usage

```rust
  // Declare static components
  use flax::*;
  component! {
    health: f32,
    regen: f32,
    pos: (f32, f32),
    player: (),
    items: Vec<String>,
  }

  let mut world = World::new();

  // Spawn an entity
  let p = EntityBuilder::new()
      .set(health(), 50.0)
      .tag(player())
      .set(pos(), (0.0, 0.0))
      .set(regen(), 1.0)
      .set_default(items())
      .spawn(&mut world);

  let mut query = Query::new((health().as_mut(), regen()));

  // Apply health regen for all match entites
  for (health, regen) in &mut query.borrow(&world) {
      *health = (*health + regen).min(100.0);
  }

```

## Systems
Queries with logic can be abstracted into a system, and multiple systems can be
collected into a schedule.

```rust



let regen_system = System::builder()
    .with(Query::new((health().as_mut(), regen())))
    .for_each(|(health, regen)| {
        *health = (*health + regen).min(100.0);
    })
    .boxed();

let despawn_system = System::builder()
    .with(Query::new(entity_ids()).filter(health().le(0.0)))
    .write::<CommandBuffer>()
    .build(|mut q: QueryBorrow<EntityIds, _>, cmd: &mut CommandBuffer| {
        for id in &mut q {
            cmd.despawn(id);
        }
    })
    .boxed();

let mut schedule = Schedule::from([regen_system, despawn_system]);

schedule.execute_par(&mut world)?;

```
## Relations

Flax provides first class many-many relations between entities, which is useful for tree scene
hierarchies, graphs, and physics joints between entities.

Relations can be both state-less or have associated data, like spring or joint strengths.

Relations are cache friendly and querying children of does not require random access. In
addition, relations are cleaned up on despawns and are stable during serialization, even if the
entity ids migrate due to collisions.
    
See [the guide](https://ten3roberts.github.io/flax/fundamentals/relations.html) for more
details.

```rust
component! {
    child_of(parent): () => [Debug],
}

let mut world = World::new();

let parent = Entity::builder()
    .set(name(), "Parent".into())
    .spawn(&mut world);

let child1 = Entity::builder()
    .set(name(), "Child1".into())
    .set_default(child_of(parent))
    .spawn(&mut world);


```


## Comparison to other ECS

Compared to other ecs implementations, a component is simply another `Entity`
identifier to which data is attached. This means the same "type" can be added to
an entity multiple times.

A limitation of existing implementations such as [specs](https://github.com/amethyst/specs), [planck](https://github.com/jojolepro/planck_ecs/), or [hecs](https://github.com/Ralith/hecs) is that newtype wrappers need to be created to allow components of the same inner type to coexist.

This leads to having to forward all trait implementations trough e.g
`derive-more` or dereferencing the newtypes during usage.

This can lead to situations such as this:

```rust,ignore
struct Position(Vec3);
struct Velocity(Vec3);

let vel = world.get::<Velocity>(entity);
let mut pos = world.get_mut::<Position>(entity);
let dt = 0.1;

*pos = Position(**pos + **vel * dt);
```

Which in Flax is:

```rust
component! {
    velocity: Vec3,
    position: Vec3,
}

let vel = world.get(entity, velocity())?;
let mut pos = world.get_mut(entity, position())?;
let dt = 0.1;

*pos = *pos + *vel * dt;
```

On a further note, since the components have to be declared beforehand (not
always true, more on that later), it limits the amount of types which can be
inserted as components. This fixes subtle bugs which come by having the type
dictate the component, such as inserting an `Arc<Type>` instead of just `Type`,
which leads to subsequent systems not finding the `Type` on the entity.

Having statically declared componenents makes the rust type system disallow
these cases and catches these bugs earlier.

## Motivation

During development of a game in school I used the `hecs` ECS. It is an awesome
library, and the author [Ralith](https://github.com/Ralith) has been awesome in bringing some pull
requests in.

Despite this, I often made subtle bugs with *similar* types. The game engine was
cluttered with gigantic newtypes for `Velocity`, `Position` with many deref
coercions in order to coexist.

## Unsafe
This library makes use of unsafe for type erasure and the allocation in storage
of ComponentBuffers and Archetypes.

<!-- cargo-sync-readme end -->

License: MIT
