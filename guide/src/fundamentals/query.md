# Query

A query is the beating heart of the ECS.

They provide a declarative method to iterate, modify, and inspect the world's entities and their components.

In short a query is a declaration of which components to access, though they allow for so much more, such as filtering or excluding entities with certain components, change detection, relationship and graph traversal and much much more.

```rust
{{ #rustdoc_include ../../../examples/guide/query.rs:query_simple }}
```

A query accepts any type which implements
[Fetch](https://docs.rs/flax/latest/flax/fetch/trait.Fetch.html), such as

- A component
- A tuple of components
- [`component.as_mut()`](https://docs.rs/flax/latest/flax/struct.Component.html#method.as_mut) for mutable access
- [`component.as_cloned()`](https://docs.rs/flax/latest/flax/struct.Component.html#method.as_cloned) for cloning the values
- [`component.opt()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt) for an optional access, returns Option<&T>,
- [`component.as_mut().opt()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt) for an optional mutable access, returns Option<&mut T>,
- [`component.opt_or_default()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt_or_default) for returning the default value if the component is not present
- [`component.opt_or()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt_or) for returning a default if the component is not present
- [`entity_ids`](https://docs.rs/flax/latest/flax/entity/fn.entity_ids.html) to return the
  iterated `Entity` ids.
- ... and many more
- 
See [Queries](../query/index.md) for more details

## Filters

A query allows for filters, such as skipping entities which have a certain
components, or where the value of a component satisfies some condition. This will efficiently skip entire archetypes or ranges of entities.

The following example shows a query which will update the distance to origin
when an entity moves for every entity.

```rust
{{ #include ../../../examples/guide/query.rs:query_modified }}
```

The same query can be run again, but since all changes have been visited, it
yields nothing.

```rust
{{ #include ../../../examples/guide/query.rs:query_repeat }}
```

However, if the position were to be modified, the query would yield that one change.

```rust
{{ #include ../../../examples/guide/query.rs:query_repeat_reboot }}
```

For situations where an `or` combined filter is used in conjunction with a fetch
of the same components, the filter may be attached directly to the query fetch
instead.

```rust
{{ #include ../../../examples/guide/query.rs:shorthand }}
```
