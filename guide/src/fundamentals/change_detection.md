# Change Detection

Flax tracks when a component is added, mutably accessed, or removed.

Queries allow filtering on change events since the query last run.

The following example creates a system which prints the updated health values
for each entity.

```rust
{{ #include ../../../examples/guide/change_detection.rs:health_changes }}
```

`Removal` events are created by
[`World::remove`](https://docs.rs/flax/latest/flax/struct.World#method.remove).


```rust
{{ #include ../../../examples/guide/query_advanced.rs:physics }}
```

## Implementation details

    Each `ChangeEvent` consists of a subslice of adjacent entities in the same
    archetype, the change type, and when the change occurred.

    Two change events where the entities are adjacent will be joined into a single
    one will be joined. This means the change list is always rather small compared
    to the number of changing entities (especially compared to using a `HashSet`).

    The following example combines optional queries with change detection to create
    a small physic calculation.
