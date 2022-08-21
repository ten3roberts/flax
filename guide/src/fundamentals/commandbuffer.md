# CommandBuffer

The commandbuffer allows deferred modification of the world.

This is useful in queries where the world is currently being iterated over, or
in other situations where a mutable reference to the world is not available.

```rust
{{ #include ../../../examples/guide/commandbuffer.rs:basics }}
```

## Usage with schedule

A schedule contains a commandbuffer which is available in systems through
`.write::<CommandBuffer>()`

```rust
{{ #include ../../../examples/guide/commandbuffer.rs:schedule }}
```

The commandbuffer will be applied at the end of the schedule automatically.

`flush` can be used to apply the commandbuffer to make the modifications visible
to the following systems.
