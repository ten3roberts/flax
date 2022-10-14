# EntityQuery

Similar to a normal query, an
[EntityQuery](https://docs.rs/flax/latest/flax/struct.EntityQuery.html) allows
simultanous access to multiple components using a fetch, but for a single,
already known entity.

This is useful when dealing with static resource entities and allowing systems
to efficiently access the resources it needs.

```rust
{{ #include ../../../examples/guide/query.rs:entity_query }}
```
