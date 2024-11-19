use std::iter::repeat;

use flax::{
    component,
    serialize::{SerializationContextBuilder, SerializeFormat},
    BatchSpawn, World,
};

use serde::{Deserialize, Serialize};

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

        let (serializer, deserializer) = SerializationContextBuilder::new()
            .with(transform())
            .with(position())
            .with(rotation())
            .with(velocity())
            .build();
        let encoded =
            ron::to_string(&serializer.serialize(world, SerializeFormat::ColumnMajor)).unwrap();

        deserializer
            .deserialize(&mut ron::Deserializer::from_str(&encoded).unwrap())
            .unwrap();
    }

    pub fn run_row(&mut self) {
        let Self(world) = self;

        let (serializer, deserializer) = SerdeBuilder::new()
            .with(transform())
            .with(position())
            .with(rotation())
            .with(velocity())
            .build();
        let encoded =
            ron::to_string(&serializer.serialize(world, SerializeFormat::RowMajor)).unwrap();

        deserializer
            .deserialize(&mut ron::Deserializer::from_str(&encoded).unwrap())
            .unwrap();
    }
}
