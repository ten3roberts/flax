# Graphs

The relation system allows creating Entity hierarchies and graphs.

These graphs can be traversed in different ways through the use of a [Query](https://docs.rs/flax/latest/flax/struct.Query.html).

The default `child_of` relation provides a *mutually exclusive* parent-child relation, which is perfect for transform hierarchies or trees, and is trivial to construct using the [EntityBuilder::attach](https://docs.rs/flax/latest/flax/struct.EntityBuilder.html#method.attach) method.

```rust
{{ #include ../../../examples/query/graphs.rs:builder }}
```

Likewise, [`attach_with`](https://docs.rs/flax/latest/flax/entity/struct.EntityBuilder.html#method.attach_with) can be used for stateful relations

## Depth First Iteration

[Dfs](https://docs.rs/flax/latest/flax/struct.Dfs.html) will traverse the subtree of the provided `root` in depth-first order.

[DfsRoots](https://docs.rs/flax/latest/flax/struct.DfsRoots.html) is similar to `Dfs`, but will traverse *all* trees, and does as such not require a starting point.

```rust
{{ #include ../../../examples/query/graphs.rs:dfs }}
```
