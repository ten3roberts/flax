use flax::*;
use glam::{Quat, Vec3};
use rand::{rngs::StdRng, Rng, SeedableRng};

#[test]
fn migrate() {
    component! {
        position: Vec3,
        rotation: Quat,
        scale: Vec3,
    }

    let mut world1 = World::new();

    let mut rng = StdRng::seed_from_u64(48);
    (0..20).for_each(|i| {
        Entity::builder()
            .set(name(), format!("a.{i}"))
            .set(position(), rng.gen())
            .spawn(&mut world1);
    });

    (0..10).for_each(|i| {
        Entity::builder()
            .set(name(), format!("a.{i}"))
            .set(position(), rng.gen())
            .set(rotation(), rng.gen())
            .spawn(&mut world1);
    });

    // Exclusive to world1
    (0..10).for_each(|i| {
        Entity::builder()
            .set(name(), format!("a.{i}"))
            .spawn(&mut world1);
    });

    let mut world2 = World::new();

    (0..20).for_each(|i| {
        Entity::builder()
            .set(name(), format!("b.{i}"))
            .set(position(), rng.gen())
            .spawn(&mut world2);
    });

    (0..10).for_each(|i| {
        Entity::builder()
            .set(name(), format!("b.{i}"))
            .set(position(), rng.gen())
            .set(rotation(), rng.gen())
            .spawn(&mut world2);
    });

    // Exclusive to world2
    (0..10).for_each(|i| {
        Entity::builder()
            .set(name(), format!("b.{i}"))
            .set(position(), rng.gen())
            .set(rotation(), rng.gen())
            .set(scale(), rng.gen())
            .spawn(&mut world2);
    });

    eprintln!("world1: {world1:#?}\n\nworld2: {world2:#?}");

    let new_ids = world1.merge_with(&mut world2);

    eprintln!("New ids: {new_ids:#?}");
    eprintln!("World: {world1:#?}");
    assert_eq!(Query::new((position())).borrow(&world2).count(), 0);
    assert_eq!(Query::new(name()).borrow(&world1).count(), 80);
}
