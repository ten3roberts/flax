# Filters
A filter allows further specifying, and consequently narrowing, the subset of entities to visit.

For instance, a filter can allow querying the set of entities which have a specified component, or the set of entities which *do not* have the specified components, among others.

## With and Without
Allows including or excluding a set of entities from the query depending on their components.

Combined with tag like components, a query which only yields the player can be achieved.

```rust
{{ #include ../../../examples/query/basic.rs:query_with }}
```

... or everything that isn't a player, that of course still has the required `name` and `health`.

```rust
{{ #include ../../../examples/query/basic.rs:query_without }}
```

## Combinators
Several filters can be combined using `&` and `|`, as well as `!`.

## Comparison
It is also possible to filter on the value of a component, using the

```rust
{{ #include ../../../examples/query/basic.rs:query_cmp }}
```
