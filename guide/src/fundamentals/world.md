# World

The world holds the entities and components in the ECS.

```rust
{{#include ../../../examples/guide.rs:world}}
```

Spawning an entity yields an `Entity` id.


```rust
{{#include ../../../examples/guide.rs:spawn}}
```

When an entity is despawned, it can no longer be accessed.

Entity ids are versioned, which means that once an entity is despawned the index
in the storage may be reused, but it will have a different version, which
prevents dead entities to become alive at a later point in time. I.e; dead
entities stay dead, this is not a zombie apocalypse we are working with.

```rust
{{#include ../../../examples/guide.rs:despawn}}
```

Many entities can be spawned at a time, which is easily demonstrated by this
iterator which takes entity ids from the world

```rust
{{#include ../../../examples/guide.rs:spawn_many}}
```

