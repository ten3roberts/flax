# Advanced query

A query is a combination of a
[`Fetch`](https://docs.rs/flax/latest/flax/traits.Fetch) and a
[`Filter`](https://docs.rs/flax/latest/flax/traits.Filter) in unison with a state.

The most common kind of `Fetch` is the component and the tuple fetch. The tuple
combines multiple fetches into a single one.

A tuple of components, or any other `Fetch` will only yield entities which match
the full tuple, I.e;

## Optional query

```rust
{{ #include ../../../examples/guide/query_advanced.rs:full_match }}
```

Will only yield entities which have both position and velocity.

By adding `.opt()` to any fetch, you convert it to an `Option<T>` where `T` is
the value yielded by the underlying fetch

```rust
{{ #include ../../../examples/guide/query_advanced.rs:opt }}
```

However, it can still be cumbersome to deal with the option, especially since
components yield references, which can not be combined with
[`Option::unwrap_or_default`](https://doc.rust-lang.org/std/option/enum.Option.html#method.unwrap_or_default)

The [`opt_or_default`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt_or_default) combinator can be used, or [`opt_or`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.opt_or) to provide your custom value if the entity does not have the specified component.

There only work for fetches which return a shared reference, as the provided
default is also returned by reference. For when there is an owned value, consider using the builting `Option::unwrap_or_default`.

This combinator is useful when writing systems which may need to operate on entities which lack some components, such as physics where the entity may not have a rotation.

## Change detection

Every time a component is modified, either through [`World::get_mut`](https://docs.rs/flax/latest/flax/struct.World#method.get_mut), or a query, a `Modified` event is added to the affected entities.

Similarly, `set` when the component did not previously exist, and new entities will create an `Added` event.

`Removal` events are created by
[`World::remove`](https://docs.rs/flax/latest/flax/struct.World#method.remove).

The following example makes use of optional combinators and change detection to
handle a 3d world.

```rust
{{ #include ../../../examples/guide/query_advanced.rs:physics }}
```

### Implementation details

    Each `ChangeEvent` consists of a subslice of adjacent entities in the same
    archetype, the change type, and when the change occurred.

    Two change events where the entities are adjacent will be joined into a single
    one will be joined. This means the change list is always rather small compared
    to the number of changing entities (especially compared to using a `HashSet`).

    The following example combines optional queries with change detection to create
    a small physic calculation.
