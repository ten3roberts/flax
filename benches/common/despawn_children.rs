use std::iter::repeat;

use flax::{components::child_of, *};

component! {
    a: f32,
    b: f32,
}

pub struct Benchmark(World, Entity);

fn spawn_children(world: &mut World, parent: Entity) {
    let a = repeat((Some(a()), None)).take(100);

    let b = repeat((None, Some(b()))).take(100);
    let ab = repeat((None, None)).take(100);

    a.chain(b).chain(ab).for_each(|(a, b)| {
        let mut builder = Entity::builder();
        builder.set(child_of(parent), ());

        if let Some(a) = a {
            builder.set(a, 0.0);
        }

        if let Some(b) = b {
            builder.set(b, 0.0);
        }

        builder.spawn(world);
    });
}

impl Benchmark {
    pub fn new() -> Self {
        let mut world = World::default();
        let parent = Entity::builder().spawn(&mut world);

        for _ in 0..100 {
            let parent = Entity::builder()
                .set(child_of(parent), ())
                .spawn(&mut world);

            spawn_children(&mut world, parent);
        }

        Self(world, parent)
    }

    pub fn run(&mut self) {
        self.0.despawn_children(self.1, child_of).unwrap();
    }
}
