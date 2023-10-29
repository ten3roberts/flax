# Dynamic components

A component is nothing more than a type safe entity id.

The [component](https://docs.rs/flax/latest/flax/macro.component.html) uses a
lazily acquired entity. It does not require the world since the entity is
spawned in the `STATIC` global namespace which is shared across all worlds.

It is entirely possible to create a component at runtime for e.g; a local system.

```rust
{{ #include ../../../examples/guide/dynamic_components.rs:custom }}
```

The `meta` allows the user to provide a function to attach extra components to
the entity. This is used by the `=> [ty, ty]` syntax for the `component` macro.

The world will automatically manage the lifetime of the component to ensure that
no entity has invalid components attached to it.

## Relations

As a component is just a normal entity you can add an entity to another entity.
Such as adding a parent entity to the children entities.

However, while this method allows quick and easy entity hierarchies, there is no
notion of what the entity represents, and no way to distinguish it from another
component.

This is where component parameterization comes to play.

An entity is `64` bits in size, sufficient for holding the `kind`, `index`, and
`generation`.

However, since the world manages the lifetime of the component, the generation
of `32` bits is freed up, which allows other data to be stored, such as another
entity id.

This allows the upper bits of a component(*entity*) id to contain another
entity as the generation is not needed.

```rust
{{ #include ../../../examples/guide/dynamic_components.rs:relation }}
```

When despawning either the relation component or target entity, the "parent",
the component is removed from all entities.
