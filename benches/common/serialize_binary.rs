use std::iter::repeat;

use bincode::Options;
use flax::{
    component,
    serialize::{SerializationContextBuilder, SerializeFormat},
    BatchSpawn, World,
};
use serde::{de::DeserializeSeed, Deserialize, Serialize};

#[derive(Default, Copy, Clone, Serialize, Deserialize)]
struct Transform([f32; 16]);

#[derive(Default, Copy, Clone, Serialize, Deserialize)]
struct Position {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Default, Copy, Clone, Serialize, Deserialize)]
struct Rotation {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Default, Copy, Clone, Serialize, Deserialize)]
struct Velocity {
    x: f32,
    y: f32,
    z: f32,
}

component! {
    transform: Transform,
    position: Position,
    rotation: Rotation,
    velocity: Velocity,
}

pub struct Benchmark(World);

impl Benchmark {
    pub fn new() -> Self {
        let mut world = World::new();

        let mut batch = BatchSpawn::new(1000);
        batch.set(transform(), repeat(Default::default())).unwrap();
        batch.set(position(), repeat(Default::default())).unwrap();
        batch.set(rotation(), repeat(Default::default())).unwrap();
        batch.set(velocity(), repeat(Default::default())).unwrap();
        batch.spawn(&mut world);

        Self(world)
    }

    pub fn run_col(&mut self) {
        let Self(world) = self;

        let context = SerializationContextBuilder::new()
            .with(transform())
            .with(position())
            .with(rotation())
            .with(velocity())
            .build();

        let encoded = bincode::options()
            .serialize(&context.serialize_world(world, SerializeFormat::ColumnMajor))
            .unwrap();

        context
            .deserialize_world()
            .deserialize(&mut bincode::Deserializer::from_slice(
                &encoded,
                bincode::options(),
            ))
            .unwrap();
    }

    pub fn run_row(&mut self) {
        let Self(world) = self;

        let context = SerializationContextBuilder::new()
            .with(transform())
            .with(position())
            .with(rotation())
            .with(velocity())
            .build();

        let encoded = bincode::options()
            .serialize(&context.serialize_world(world, SerializeFormat::RowMajor))
            .unwrap();

        context
            .deserialize_world()
            .deserialize(&mut bincode::Deserializer::from_slice(
                &encoded,
                bincode::options(),
            ))
            .unwrap();
    }
}
