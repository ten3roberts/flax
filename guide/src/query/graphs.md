# Graphs

The relation system allows creating Entity hierarchies and graphs.

These graphs can be traversed in different ways through the use of a [Query](https://docs.rs/flax/latest/flax/struct.Query.html).

The default `child_of` relation provides a *mutually exclusive* parent-child relation, which is perfect for transform hierarchies or trees, and is trivial to construct using the [EntityBuilder::attach](https://docs.rs/flax/latest/flax/struct.EntityBuilder.html#method.attach) method.

```rust
{{ #include ../../../examples/query/graphs.rs:builder }}
```

Likewise, [`attach_with`](https://docs.rs/flax/latest/flax/entity/struct.EntityBuilder.html#method.attach_with) can be used for stateful relations

## Depth First Iteration

[Dfs](https://docs.rs/flax/latest/flax/query/struct.Dfs.html) allows traversing hierarchies in depth-first order.

```rust
{{ #include ../../../examples/query/graphs.rs:dfs }}
```

### Traversal

For modifying a graph through a value which is passed through the parents, such as updating a UI layout, there is [Dfs::traverse](https://docs.rs/flax/latest/flax/query/struct.Dfs.html#method.traverse) which provides an easy recursion based visiting, which can for example be used for updating transform hierarchies.

Example:

```rust
{{ #include ../../../examples/query/transform.rs:systems }}
```

See: [transform](https://github.com/ten3roberts/flax/blob/main/examples/query/transform.rs)

## Topological Iteration

In addition to *depth first* traversal, queries offer iteration in topological ordering through [Topo](https://docs.rs/flax/latest/flax/query/struct.Topo.html).

Topological ordering is less constrained as it only ensures that each node's parents are visited before the children, but not that the children are visited immediately after the parent.

More specifically, each *node* is visited in the ordered of their *maximum recursion depth*. I.e, first all roots are visited, then all children, then all *2nd* level children and so on.

This allows far greater cache locality and is more similar in memory access patterns to the non-relation aware [Planar](https://docs.rs/flax/latest/flax/query/struct.Planar.html) strategy.
