#![allow(clippy::new_without_default)]
use std::iter::repeat;

use flax::*;
use glam::*;

component! {
    transform: Mat4,
    position: Vec3,
    rotation: Vec3,
    velocity: Vec3,
}

#[test]
pub fn simple_iter() {
    let mut world = World::new();

    let mut batch = BatchSpawn::new(10000);
    batch
        .set(transform(), repeat(Mat4::from_scale(Vec3::ONE)))
        .unwrap();

    batch.set(position(), repeat(Vec3::X)).unwrap();
    batch.set(rotation(), repeat(Vec3::X)).unwrap();
    batch.set(velocity(), repeat(Vec3::X)).unwrap();

    batch.spawn(&mut world);
    let mut query = Query::new((velocity(), position().as_mut()));

    for (velocity, position) in &mut query.borrow(&world) {
        *position += *velocity
    }
}
