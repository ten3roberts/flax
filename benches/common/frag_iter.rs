use flax::*;
use std::iter::repeat;

component! {
    data: f32,
}

macro_rules! create_entities {
    ($world:ident; $( $variants:ident ),*) => {
        $(
            component! { $variants: f32, };

            let mut batch = BatchSpawn::new(20);
            batch.set($variants(), repeat(0.0)).unwrap();
            batch.set(data(), repeat(0.0)).unwrap();
            batch.spawn(&mut $world);
        )*
    };
}

pub struct Benchmark(World);

impl Benchmark {
    pub fn new() -> Self {
        let mut world = World::default();

        create_entities!(world; a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x, y, z);

        Self(world)
    }

    pub fn run(&mut self) {
        for data in &mut Query::new(data().as_mut()).borrow(&self.0) {
            *data *= 2.0;
        }
    }

    pub fn run_for_each(&mut self) {
        Query::new(data().as_mut())
            .borrow(&self.0)
            .for_each(|data| {
                *data *= 2.0;
            })
    }

    pub fn run2(&mut self) {
        for data in &mut Query::new(data().as_mut()).borrow(&self.0) {
            *data *= 2.0;
        }
    }

    pub fn run_for_each2(&mut self) {
        Query::new(data().as_mut())
            .borrow(&self.0)
            .for_each(|data| {
                *data *= 2.0;
            })
    }
}
