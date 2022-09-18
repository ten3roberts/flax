use flax::*;
use std::iter::repeat;

fn ab_system() -> BoxedSystem {
    System::builder()
        .with(Query::new((a().as_mut(), b().as_mut())))
        .for_each(|(a, b)| {
            std::mem::swap(a, b);
        })
        .boxed()
}

fn cd_system() -> BoxedSystem {
    System::builder()
        .with(Query::new((c().as_mut(), d().as_mut())))
        .for_each(|(c, d)| {
            std::mem::swap(c, d);
        })
        .boxed()
}

fn ce_system() -> BoxedSystem {
    System::builder()
        .with(Query::new((c().as_mut(), e().as_mut())))
        .for_each(|(c, e)| {
            std::mem::swap(c, e);
        })
        .boxed()
}

component! {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
}

#[test]
fn schedule_granularity() {
    let mut world = World::default();

    let mut batch = BatchSpawn::new(10_000);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let mut batch = BatchSpawn::new(10_000);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.set(c(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let mut batch = BatchSpawn::new(10_000);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.set(c(), repeat(0.0)).unwrap();
    batch.set(d(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let mut batch = BatchSpawn::new(10_000);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.set(c(), repeat(0.0)).unwrap();
    batch.set(e(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let mut schedule = Schedule::builder()
        .with_system(ab_system())
        .with_system(cd_system())
        .with_system(ce_system())
        .build();

    let batches = schedule.batch_info(&mut world);

    eprintln!("Batches: {batches:#?}");
    assert_eq!(batches.len(), 1);

    schedule.execute_par(&mut world).unwrap();
}
