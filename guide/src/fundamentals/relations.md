# Relations

A relation is a component which *links* to another `Entity`, similar to a foreign key in a database.

The linked entity is referred to as the `object` of a relation, while the entity the component is attached to is called the `subject`.

This allows forming hierarchies such as *parent-child* relations for transforms and UI, as well as arbitrary graphs.

A relation is used as a *parameterized* component, which requires an `Entity` to be fully instantiated.

Relations are most easily declared using the
[component](https://docs.rs/flax/latest/flax/macro.component.html) macro.

```rust
{{ #include ../../../examples/guide/relations.rs:relation_basic }}
```

Important to note is that the same `child_of` component with different `object`
arguments are distinct, and can as such exist on an entity at the same time,
allowing many-many relationships between entities;

There is no limitation of the number of relations an entity can have. As such,
an entity can have multiple relations to other entities, allowing for any kind of graphs inside the ecs.

```rust
{{ #include ../../../examples/guide/relations.rs:many_to_many }}
```

## Queries

Since relations are normal components, they can be used in a query as normal, or
used to exclude components.

See the [Graphs](../query/graphs.md) chapter in queries.

```rust
{{ #include ../../../examples/guide/relations.rs:query }}
```

## Lifetime

When an entity is despawned, all relations to it present on other components
will be removed and dropped. As such, no entity will have a relation to an
entity which does not exist.

```rust
{{ #include ../../../examples/guide/relations.rs:lifetime }}
```
