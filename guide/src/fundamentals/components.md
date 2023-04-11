# Components

A component represents data which is attached to an entity.

**Note**: Compared to other Rust ECS implementations, a component is not the
same as the underlying type. This allows different components of the same
_datatype_ to coexist without having to use newtypes and forward all traits, and
implement `Into` and `From` implementations.

```rust
{{ #include ../../../examples/guide.rs:component_decl }}
```

This in turn exposes a function that will return the component id, as component ids
are lazily allocated due to lack of compile time unique ids.

The component can be added, accessed and removed from entities using
[World::set](https://docs.rs/flax/latest/flax/struct.World.html#method.set),
[World::get](https://docs.rs/flax/latest/flax/struct.World.html#method.get), and
[World::remove](https://docs.rs/flax/latest/flax/struct.World.html#method.remove).

```rust
{{ #include ../../../examples/guide.rs:world_component_access }}
```

Accessing a component mutably does not require a mutable access to the world, as
it uses an AtomicRefCell.

Multiple _different_ components can be accessed simultaneously, even on the same
entity.

## Default components

Flax provides some opinionated default components to ease the communication
between different libraries and users.

- [name](https://docs.rs/flax/latest/flax/components/fn.name.html): Provides a name for entities and components.
- [child_of](https://docs.rs/flax/latest/flax/components/fn.child_of.html): Default dataless hierarchy relation. See: [Relations](https://ten3roberts.github.io/flax/guide/fundamentals/relations.html)
