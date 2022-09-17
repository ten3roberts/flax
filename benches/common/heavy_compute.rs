use std::{iter::repeat, sync::atomic::AtomicUsize};

use flax::*;
use glam::*;
use rayon::prelude::*;

component! {
    mat: Mat4,
    position: Vec3,
    rotation: Vec3,
    velocity: Vec3,
}

#[derive(Copy, Clone)]
struct Position(Vec3);

#[derive(Copy, Clone)]
struct Rotation(Vec3);

#[derive(Copy, Clone)]
struct Velocity(Vec3);

pub struct Benchmark(World);

impl Benchmark {
    pub fn new() -> Self {
        let mut world = World::default();

        let mut batch = BatchSpawn::new(1000);
        batch
            .set(mat(), repeat(Mat4::from_axis_angle(Vec3::X, 1.2)))
            .unwrap()
            .set(position(), repeat(Vec3::X))
            .unwrap()
            .set(rotation(), repeat(Vec3::X))
            .unwrap()
            .set(velocity(), repeat(Vec3::X))
            .unwrap()
            .spawn(&mut world);

        Self(world)
    }

    pub fn run(&mut self) {
        Query::new((position().as_mut(), mat().as_mut()))
            .batch_size(64)
            .borrow(&self.0)
            .iter_batched()
            .par_bridge()
            .for_each(|batch| {
                for (pos, mat) in batch {
                    for _ in 0..100 {
                        *mat = mat.inverse();
                    }

                    *pos = mat.transform_vector3(*pos);
                }
            });
    }

    pub fn run_seq(&mut self) {
        Query::new((position().as_mut(), mat().as_mut()))
            .borrow(&self.0)
            .iter()
            .for_each(|(pos, mat)| {
                for _ in 0..100 {
                    *mat = mat.inverse();
                }

                *pos = mat.transform_vector3(*pos);
            });
    }
}
