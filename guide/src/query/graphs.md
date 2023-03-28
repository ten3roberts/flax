# Graphs

The relation system allows creating Entity hierarchies and graphs.

These graphs can be traversed in different ways through the use of a [Query](https://docs.rs/flax/latest/flax/struct.Query.html).

The default `child_of` relation provides a *mutually exclusive* parent-child relation, which is perfect for transform hierarchies or trees, and is trivial to construct using the [EntityBuilder::attach](https://docs.rs/flax/latest/flax/struct.EntityBuilder.html#method.attach) method.

```rust
{{ #include ../../../examples/query/graphs.rs:builder }}
```

Likewise, [`attach_with`](https://docs.rs/flax/latest/flax/entity/struct.EntityBuilder.html#method.attach_with) can be used for stateful relations

## Depth First Iteration

[Dfs](https://docs.rs/flax/latest/flax/struct.Dfs.html) allows traversing hierarchies in depth-first order.

```rust
{{ #include ../../../examples/query/graphs.rs:dfs }}
```

### Traversal

In addition to plain iteration, the `Dfs` allows you
