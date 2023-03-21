# Entity Query

By default, a query will iterate all entities which match the archetype.

However, the query strategy can be changed to only return a single entity, which is useful for queries over a resource entity or player.


```rust
{{ #include ../../../examples/guide/query.rs:entity_query }}
```

In addition, an entity query can be used in a system

```rust
{{ #include ../../../examples/guide/query.rs:entity_query_system }}

```
