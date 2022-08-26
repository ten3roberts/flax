# Entity Builder

The [EntityBuilder](https://docs.rs/flax/latest/flax/struct.EntityBuilder.struct)
allows you to incrementally construct an entity by adding components and *then*
inserting it into the world. This provides both better ergonomics and efficiency
as the entity does not bounce around archetypes.

Additionally, the entity builder allows constructing an entity in the absence of
the world, like a function which only creates the `EntityBuilder`.

```rust
{{ #include ../../../examples/guide/builder.rs:builder }}
```

When the entity builder is spawned, the held components are moved out into the
matching archetype in the world.

This means the entity builder is cleared and ready to insert more components.

```rust
{{ #include ../../../examples/guide/builder.rs:reuse }}
```

```rust
{{ #include ../../../examples/guide/builder.rs:query }}
```

## Batch Spawn

If inserting **many** entities of the same type, it is more efficient to let the world know about
the coming components and insert everything at once.

The batch spawn allows spawning many entities with the same component types at
once by inserting columns of each component type. This is akin to the native
format of the archetype storage and is thus maximally efficient.

```rust
{{ #include ../../../examples/guide/builder.rs:batch }}
```

## Hierarchy

In the previous chapter, hierarchies were constructed by adding a relation to an
entity to the child entity.

The entity builder allows construction of hierarchies before being spawned into
the world.

```rust
{{ #include ../../../examples/guide/builder.rs:hierarchy }}
```
