# Component metadata

The keen eyed of you may have noticed that `ComponentId` is the same as
`Entity`.

This is a design choice; a component is also an entity, and as such, exists in
the world.

This brings some interesting possibilities which are not possible in other ECS
systems, mainly: components can have components.

This allows the components to be queried just as they were normal entities, which
allows reflection.

For example, a component can itself have a component which knows how to
`Debug::fmt` the component value, another component could be used to serialize a
value.

While components could be added to components through the conventional
[`World::set`] syntax, it can quickly become a spaghetti of `init` functions for
each library to add the required components, or metadata to the exported
components.

This is where the [component](https://docs.rs/flax/latest/flax/macro.component.html) macro comes into play. The component function acquires a globally free `Entity` and assigns that to the strongly typed component.
When the component is first inserted into the world it can insert so called *metadata* to the component.


```rust
{{ #include ../../../examples/guide.rs:component_meta }}
```

The
```rust
component: type => [ Meta, Meta, Meta ]
```
syntax is used to add metadata to the component.

The `component` component, and the `name` component is always present.

## Debug

If a component has the
[flax::Debuggable](https://docs.rs/flax/latest/flax/struct.Debug.html) component, the
component and value for each entity will be present when debug formatting the
world.

## Name

Every component has a `name`, which is the same as the declaration.

## Custom metadata

Custom metadata can be added to a component by creating a struct which
implements [MetaData](https://docs.rs/flax/latest/flax/trait.MetaData.html),
which is responsible of adding the appropriate components.

**Note**: Do not use conditional compilation for these structs as this leaks the
`cfg` directives to any uses of the metadata in the `component` macro. Instead,
conditionally add, or do not add, the component in the `attach` method.

A common example of this is `serde`, prefer to not add the `serializer`
component and still define `Serialize` and making it no-op when `serde` is not
enabled.
