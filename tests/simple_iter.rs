#![allow(clippy::new_without_default)]
use std::iter::repeat;

use flax::*;
use glam::*;
use itertools::Itertools;
use pretty_assertions::assert_eq;

component! {
    transform: Mat4,
    position: Vec3,
    rotation: Vec3,
    velocity: Vec3,
}

#[test]
pub fn simple_iter() {
    let mut world = World::new();

    let mut batch = BatchSpawn::new(1000);
    batch
        .set(transform(), repeat(Mat4::from_scale(Vec3::ONE)))
        .unwrap();

    batch
        .set(position(), (0..).map(|i| Vec3::Y * i as f32))
        .unwrap();
    batch.set(rotation(), repeat(Vec3::X)).unwrap();
    batch.set(velocity(), repeat(Vec3::X)).unwrap();

    batch.spawn(&mut world);
    let mut query = Query::new((velocity(), position().as_mut()));

    for (&velocity, position) in &mut query.borrow(&world) {
        *position += velocity * 0.5
    }

    assert_eq!(
        Query::new(position().copied()).collect_vec(&world),
        (0..1000)
            .map(|i| Vec3::Y * i as f32 + Vec3::X * 0.5)
            .collect_vec()
    );
}
