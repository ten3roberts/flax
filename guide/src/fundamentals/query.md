# Query

Queries allow efficient access and iteration over multiple components.

They allow iterating entities which have a certain set of components, and match
a certain filter.

```rust
{{ #include ../../../examples/guide/query.rs:query_simple }}
```

A query accepts any type which implements
[Fetch](https://docs.rs/flax/latest/flax/fetch/trait.Fetch.html):

- A component
- A tuple of components
- `component.as_mut()` for mutable access
- `component.opt()` for an optional access, returns Option<&T>,
- `component.as_mut().opt()` for an optional mutable access, returns Option<&mut T>,
- `component.opt_or_default()` for an access which returns a default value if
  the current entity does not have the component.
- [entities](https://docs.rs/flax/latest/flax/fn.entities.html) to return the
  iterated `Entity` ids.
- ... and many more

# Filters

A query allows for filters, such as skipping entities which have a certain
components, or where the value of a component satisfies some condition.

Change detection is also supported and allows querying only over the entities
where a certain component changed.

The following example shows a query which will update the distance to origin
when an entity moves for every entity which are not dead.

```rust
{{ #include ../../../examples/guide/query.rs:query_modified }}
```
