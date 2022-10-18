# Relations

Relations such as hierarchies or entity-entity connections are a common way to structure entities in an application or game,
such as nested UI elements or entities in a game world which move along with
their parents.

Flax has first class support for hierarchies, called `Relations`.

A relation in Flax is represented by a parameterized component containing the
`object`, i.e; parent added to the `subject`, i.e; child entity.

Think of it as the component accepting an argument entity prior to adding it to
another entity.

Relations are declared using the
[component](https://docs.rs/flax/latest/flax/macro.component.html) macro

```rust
{{ #include ../../../examples/guide/relations.rs:relation_basic }}
```

Important to note is that the same `child_of` component with different `object`
arguments are distinct, and can as such exist on an entity at the same time,
allowing many-many relationsships between entities;

There is no limatation of the number of relations an entity can have. As such,
an entity can have multiple relations to other entities, allowing for any kind of graphs inside the ecs.

```rust
{{ #include ../../../examples/guide/relations.rs:many_to_many }}
```

## Queries

Since relations are normal components, they can be used in a query as normal, or
used to exclude components.

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
