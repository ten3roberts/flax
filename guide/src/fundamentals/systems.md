# Systems

Maintaining queries and their associated data and logic can be verbose and error prone.

*Systems* provide the perfect aid and allows bundling arguments, such as
queries, world access, and much more along with the logic to execute with it.

A system can be run manually or in a [`Schedule`](https://docs.rs/flax/0.4.0/flax/schedule/struct.Schedule.html), which will automatically parallelize the system execution on multiple threads.

For example, to update `distance` from each `position` you could do:

```rust
{{ #include ../../../examples/guide/systems.rs:system_basic }}
```

The system construction can subsequently be extracted to a function

```rust
{{ #include ../../../examples/guide/systems.rs:system_func }}
```

However, the query won't yield entities since none have the `distance` component to modify. No to fear, we could add another system which ensures the component is present.

```rust
{{ #include ../../../examples/guide/systems.rs:add_missing }}
```

## For each

Most systems iterate each item of a single query, without any other system arguments.

The shorthand `for_each` is provided for this purpose

```rust
{{ #include ../../../examples/guide/systems.rs:for_each }}
```

## Schedule

Lets add some more systems and add them to a [`Schedule`](https://docs.rs/flax/0.4.0/flax/schedule/struct.Schedule.html)

```rust
{{ #include ../../../examples/guide/systems.rs:schedule }}

```

## System access

One of the abilities of a system is to automatically extract what they need for their arguments from the schedule context.

This both increases their ease of use, but further assures that only the declared pieces of data will be accessed, which allows seamless fine-grained parallelization capabilities

Therefore, it is advised to keep your system arguments as *specific* as possible, such as using queries whenever possible, rather than the whole world.

**Note**: Systems which access the world will not paralellize with other systems
as it may access anything.


```rust
{{ #include ../../../examples/guide/query.rs:system_cmd }}
```
