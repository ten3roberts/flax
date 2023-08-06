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
- [`component.as_mut()`](https://docs.rs/flax/latest/flax/struct.Component.html#method.as_mut) for mutable access
- [`component.as_cloned()`](https://docs.rs/flax/latest/flax/struct.Component.html#method.as_cloned) for cloning the values
- [`component.opt()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt) for an optional access, returns Option<&T>,
- [`component.as_mut().opt()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt) for an optional mutable access, returns Option<&mut T>,
- [`component.opt_or_default()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt_or_default) for an access which returns a default value if
- [`component.opt_or()`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt_or) for an access which returns a provided value if
  the current entity does not have the component.
- [entities](https://docs.rs/flax/latest/flax/fn.entities.html) to return the
  iterated `Entity` ids.
- ... and many more

## Filters

A query allows for filters, such as skipping entities which have a certain
components, or where the value of a component satisfies some condition.

The difference between using a query filter and a filter on the iterator is that
mutable components are not marked as modified if skipped by a query iterator.
This is because the `QueryIter` is not able to determine if a later `filter`
skipped the item. In addition, the query filters operate on ranges and can look
up modifications and alike for a group of entities, E.g; if all entities next to
each other are modified, only one range will be yielded, instead of each entity
separately.

**Note**: It is not possible to access a component mutably and filter on it at
the same time.

Change detection is also supported and allows querying only over the entities
where a certain component changed.

The following example shows a query which will update the distance to origin
when an entity moves for every entity which are not despawned.

```rust
{{ #include ../../../examples/guide/query.rs:query_modified }}
```

The same query can be run again, but since all changes have been visited, it
yields nothing.

```rust
{{ #include ../../../examples/guide/query.rs:query_repeat }}
```

However, if the position were to be modified, the query would pick up that, and
only that change.

```rust
{{ #include ../../../examples/guide/query.rs:query_repeat_reboot }}
```

For situations where an `or` combined filter is used in conjunction with a fetch
of the same components, the filter may be attached directly to the query fetch
instead.

```rust
{{ #include ../../../examples/guide/query.rs:shorthand }}
```

### Change detection

- [modified](https://docs.rs/flax/latest/flax/struct.Component.html#method.modified) yields components which have been updated **or** added.
- [inserted](https://docs.rs/flax/latest/flax/struct.Component.html#method.added) yields new components.
- [removed](https://docs.rs/flax/latest/flax/struct.Component.html#method.removed) yields each entity for which the component was recently removed.

All change detection is per query and based on when the query last executed.

### Comparative filter

In addition to change detection, filtering on value is also possible. See: [CmpExt](https://docs.rs/flax/latest/flax/trait.CmpExt.html).
