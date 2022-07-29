# Introduction

Flax is an easy to use Entity Component System.

## What is an ECS

ECS, or Entity Component System is a design paradigm of where the state of the
program is structured around multiple *Entities*, where each entity may have
zero or more components attached to it.

Systems execute upon the entities and their components.

The main benefit of this priniciple is that the logic is separate from the data,
and new functionality can be added to existing entities and components.

## How it works

In Flax, there are 3 fundamental building blocks.

[Entity](https://docs.rs/flax/latest/flax/struct.Entity.html). A unique identifier for the objects of the program. Has a managed lifecycle.

[Component](https://docs.rs/flax/latest/flax/struct.Component.html), data which
can be added to an Entity. Has a unique Id, which works as the key for storing
and retrieving the value, and a strongly typed value.

[System](https://docs.rs/flax/latest/flax/struct.System.html) functions which
execute on the world or a group of entities. Provides the logic of the program.
