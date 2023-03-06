# Basics
At its simplest form, a query is composed of a [Fetch](https://docs.rs/flax/latest/flax/fetch/trait.Fetch.html) which specify the group of components to select. The query will visit the subset of entities in the world which have the specified components, and return their values.

In order to execute a query, it has borrow the relevant data from the world. This returns a [QueryBorrow](https://docs.rs/flax/latest/flax/struct.QueryBorrow) which can then be iterated.


```rust
{{ #include ../../../examples/query/basic.rs:query_name }}
```

A `tuple` can be used which will yield the entities with all the specified components, such as all entities with a `name`, `position`, and `health`; which excludes all the pretty rocks.

```rust
{{ #include ../../../examples/query/basic.rs:query_tuple }}
```

## Mutation
[`.as_mut`](https://docs.rs/flax/latest/flax/struct.Component.html#method.as_mut) will transform a component into yielding mutable references.

```rust
{{ #include ../../../examples/query/basic.rs:query_mut }}
```

## Optional

[`.opt`](https://docs.rs/flax/latest/flax/fetch/trait.FetchExt.html#method.opt) makes a part of the fetch optional. This can be applied to a single component, or to the tuple or nested tuples as a whole.

In addition, [`.opt_or`](https://docs.rs/flax/latest/flax/fetch/trait.FetchExt.html#method.opt), and [`.opt_or_default`](https://docs.rs/flax/latest/flax/fetch/trait.FetchExt.html#method.opt) allows specifying a fallback if the entity does not have the component.

```rust
{{ #include ../../../examples/query/basic.rs:query_opt }}
```
