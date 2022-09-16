use std::iter::repeat;

use flax::*;

component! {
    a: f32,
    b: f32,
}

pub struct Benchmark(World, Vec<Entity>);

impl Benchmark {
    pub fn new() -> Self {
        let mut world = World::default();
        let mut batch = BatchSpawn::new(10000);

        batch.set(a(), repeat(0.0)).unwrap();
        let ids = batch.spawn(&mut world);

        Self(world, ids)
    }

    pub fn run(&mut self) {
        for id in &self.1 {
            self.0.set(*id, b(), 0.0).unwrap();
        }

        for id in &self.1 {
            self.0.remove(*id, b()).unwrap();
        }
    }

    pub fn run_alt(&mut self) {
        for id in &self.1 {
            self.0.set(*id, b(), 0.0).unwrap();
        }

        for id in &self.1 {
            self.0.remove(*id, b()).unwrap();
        }
    }
}
