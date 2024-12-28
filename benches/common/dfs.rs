use std::iter::repeat;

use flax::{components::child_of, filter::All, *};

component! {
    a: f32,
    b: f32,
}

type BenchmarkQuery = (EntityIds, Opt<Component<f32>>, Opt<Component<f32>>);

pub struct Benchmark(World, Query<BenchmarkQuery, All, Dfs<()>>);

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

        Self(
            world,
            Query::new((entity_ids(), a().opt(), b().opt())).with_strategy(Dfs::new(child_of)),
        )
    }

    pub fn run(&mut self) {
        let mut query = self.1.borrow(&self.0);
        query.traverse(
            &0.0,
            |(_id, a, b): (Entity, Option<&f32>, Option<&f32>), _: Option<&()>, depth: &f32| {
                a.copied().unwrap_or_default() + b.copied().unwrap_or_default() + *depth
            },
        );
    }
}
