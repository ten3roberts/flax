# Relations

A component is nothing more than a type safe entity id.

The [component](https://docs.rs/flax/latest/flax/macro.component.html) uses a
lazily acquired entity. It does not require the world since the entity is
spawned in the `STATIC` global namespace which is shared across all worlds.

It is entirely possible to create a component yourself for e.g; a local system.

```rust
{{ #include ../../../examples/guide/relations.rs:custom }}
```
