# Relations

A relation is a component which *links* to another `Entity`, similar to a foreign key in a database.

The linked entity is referred to as the `object` of a relation, while the entity the component is attached to is called the `subject`.

This allows forming hierarchies such as *parent-child* relations for transforms and UI, as well as arbitrary graphs.

A relation is used like a component which takes an `Entity` to be constructed.

Relations are most easily declared using the
[component](https://docs.rs/flax/latest/flax/macro.component.html) macro, but can be constructed dynamically as well. See [dynamic_components](../diving_deeper/dynamic_components.md)

For example, declaring a child relationship that connects to a parent can be done like so:

```rust
{{ #include ../../../examples/guide/relations.rs:relation_basic }}
```

The function like `(id)` parameter is what separates a component from a relation. The name of the parameter does not matter, and is only used for hover documentation. 

Since the value of the relation in this case is `()`, `set_default` can be used as a shorthand over `set`

Two relations of the same type but with different *objects* behave like two separate components and will not interfere. This allows having many-to-many relationships between entities, if so desired.

This allows constructing many different kinds of graphs inside the ECS.

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
## Associated values

In addition to linking between entities, a relation can also store additional data just like a component. This can be used to create weighted graphs or storing other additional information such as physical joint parameters

The following shows a more complete example of how to traverse and calculate the forces between entities connected via springs using hook's law.

```rust
{{ #include ../../../examples/guide/springs.rs:main }}
```

## Lifetime

Relations are managed by the ECS and will automatically be cleaned up. When an entity is despawned all relations which reference it will be removed from the ECS. As such, a relation will never point to an invalid entity.

```rust
{{ #include ../../../examples/guide/relations.rs:lifetime }}
```
