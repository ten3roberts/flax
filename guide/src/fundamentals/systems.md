# Systems

Having to both keep queries around, and their logic is tedious and error prone.

Systems aid in organizing your applications logic and required data.

Each system represents a set of data, like queries or view into the world, or a
commandbuffer, and a function which will be executed.

In the previous chapter, the "update distance from positions", which was reused
thrice could be turned into a query.

Instead of:

```rust
{{ #include ../../../examples/guide/query.rs:query_modified }}
```

We get this:

```rust
{{ #include ../../../examples/guide/query.rs:system_basic }}
```

## For each

If the systems sole purpose it to execute an action for each element, the
`for_each` shorthand can be used. This is only possible if the system uses a
single query.

```rust
{{ #include ../../../examples/guide/query.rs:system_for_each }}
```

## System access
Contrary to using a query directly, the `world` is not needed to prepare a
query. This is because the world is contained inside the `QueryData` argument.

This ensures a system will only access the components in the associated queries,
which allows for paralellizing system execution.

If access to the whole world is required, use `with_world`.

Similarly, `with_cmd` allows access to the commandbuffer in the system, which is
used for deferred sets, removes, or spawns.

**Note**: Systems which access the world will not paralellize with other systems
as it may access anything.


```rust
{{ #include ../../../examples/guide/query.rs:system_cmd }}
```
