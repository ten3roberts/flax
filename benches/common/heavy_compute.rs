use core::iter::repeat;

use flax::*;
use glam::*;

component! {
    mat: Mat4,
    position: Vec3,
    rotation: Vec3,
    velocity: Vec3,
}

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
            .par_for_each(|(pos, mat)| {
                for _ in 0..100 {
                    *mat = mat.inverse();
                }

                *pos = mat.transform_vector3(*pos);
            });
    }

    pub fn run_seq(&mut self) {
        Query::new((position().as_mut(), mat().as_mut()))
            .borrow(&self.0)
            .for_each(|(pos, mat)| {
                for _ in 0..100 {
                    *mat = mat.inverse();
                }

                *pos = mat.transform_vector3(*pos);
            });
    }
}
