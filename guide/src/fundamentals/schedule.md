# Schedule

A schedule allows execution and paralellization of multiple systems.

The systems discussed in the previous chapter can be put into a schedule to
contain the logic and order of execution.

In addition to executing systems one after another, a schedule can automatically
paralellize execution and run multiple systems at the same time using
[rayon](https://docs.rs/rayon/latest/rayon/) such that the observable effects
occurr order.

In other words, if two systems have queries which do not access the same
archetype and components, they will run in paralell. If an archetype reads a
value which is written by another system declared before, they will run in
sequence.


```rust
{{ #include ../../../examples/guide/query.rs:schedule_basic }}
```
