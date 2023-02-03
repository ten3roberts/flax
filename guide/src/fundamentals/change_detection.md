# Change Detection

Flax tracks when a component is added, mutably accessed, or removed.

Queries allow filtering on change events since the query last run.

- [`modified`](https://docs.rs/flax/latest/flax/struct.Component.html#method.modified) filter mutated or new components
- [`inserted`](https://docs.rs/flax/latest/flax/struct.Component.html#method.modified) only new components
- [`removed`](https://docs.rs/flax/latest/flax/struct.Component.html#method.modified) filter recently removed components.

The modified filter is best used for queries which calculate or update a value
based on one or more components, or in other ways react to a changed value.

The following example creates a system which prints the updated health values
for each entity.

A filter can be added *inline* as part of the fetch of a component, or as a filter.

**Note**: Tuple queries combine using `and`, which means a query with multiple `modified` or other change filtered components will only yield if **all** the annotated components changed since the last query ran.

Prefer using `.filter(a().modified | b().modified())` when dealing with multiple
change filters, or splitting up the query.

```rust
{{ #include ../../../examples/guide/change_detection.rs:health_changes }}
```

# Combining filters

Change filters can be combined with other filters, which leads to queries which
do even less work than that particular group member.

The following example creates a query which removes despawns entities when their
health becomes `0`. Noteworthy in particular, is that this system can run in
parallel with the previously discussed system, since they do not overlap in
mutable access.

```rust
{{ #include ../../../examples/guide/change_detection.rs:cleanup_system }}
```


# Bringing it all together

In order for the health monitoring and cleanup systems to be effective, there
needs to be something to modify the health of entities.

Such as a random damage system, and a poison status effect.

```rust
{{ #include ../../../examples/guide/change_detection.rs:damage }}
```

Using a schedule allows for easy parallelization and execution of the systems, but
is not a requirement for change detection.

```rust
{{ #include ../../../examples/guide/change_detection.rs:schedule }}
```

See the full example [here](https://github.com/ten3roberts/flax/blob/main/examples/guide/change_detection.rs)

## Implementation details

Each `ChangeEvent` consists of a subslice of adjacent entities in the same
archetype, the change type, and when the change occurred.

Two change events where the entities are adjacent will be joined into a single
one will be joined. This means the change list is always rather small compared
to the number of changing entities (especially compared to using a `HashSet`).

The following example combines optional queries with change detection to create
a small physic calculation.
