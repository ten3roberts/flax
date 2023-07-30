# Change Detection

Flax tracks when a component is added, mutably accessed, or removed.

A query allows filtering the entities based on a change event since it last ran.

- [`modified`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.modified) filter mutated or new components
- [`added`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.added) only new components
- [`removed`](https://docs.rs/flax/latest/flax/trait.FetchExt.html#method.removed) filter recently removed components.

The modified filter is best used for queries which calculate or update a value
based on one or more components, or in other ways react to a changed value.

A change filter can be added to a single component, or to a tuple of components. Applying a `.modified()` transform on a tuple will create a query which yields if *any* of the constituents were modified.

The following example creates a system which prints the updated health values
for each entity.

```rust
{{ #include ../../../examples/guide/change_detection.rs:health_changes }}
```

# Combining filters

Change filters can be combined with other filters, which leads to queries needing to perform even even less work.

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
