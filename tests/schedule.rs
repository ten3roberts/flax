use flax::{
    name, BoxedSystem, CommandBuffer, Entity, FetchExt, Query, QueryBorrow, Schedule, System, World,
};
use itertools::Itertools;

#[test]
#[cfg(feature = "rayon")]
fn schedule_granularity() {
    use flax::*;
    use std::iter::repeat;

    fn ab_system() -> BoxedSystem {
        System::builder()
            .with_query(Query::new((a().as_mut(), b().as_mut())))
            .for_each(|(a, b)| {
                std::mem::swap(a, b);
            })
            .boxed()
    }

    fn cd_system() -> BoxedSystem {
        System::builder()
            .with_query(Query::new((c().as_mut(), d().as_mut())))
            .for_each(|(c, d)| {
                std::mem::swap(c, d);
            })
            .boxed()
    }

    fn ce_system() -> BoxedSystem {
        System::builder()
            .with_query(Query::new((c().as_mut(), e().as_mut())))
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
    let mut world = World::default();

    let mut batch = BatchSpawn::new(100);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let mut batch = BatchSpawn::new(100);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.set(c(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let mut batch = BatchSpawn::new(100);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.set(c(), repeat(0.0)).unwrap();
    batch.set(d(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let mut batch = BatchSpawn::new(100);
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

    let batches = schedule.batch_info(&world);

    assert_eq!(batches.len(), 1);

    let mut batch = BatchSpawn::new(100);
    batch.set(a(), repeat(0.0)).unwrap();
    batch.set(b(), repeat(0.0)).unwrap();
    batch.set(c(), repeat(0.0)).unwrap();
    batch.set(d(), repeat(0.0)).unwrap();
    batch.set(e(), repeat(0.0)).unwrap();
    batch.spawn(&mut world);

    let batches = schedule.batch_info(&world);
    let names = batches.to_names();

    assert_eq!(batches.len(), 2, "{names:#?}");
    schedule.execute_par(&mut world).unwrap();
}

#[test]
fn command_flushing() {
    fn produce(name: &'static str) -> BoxedSystem {
        System::builder()
            .with_cmd_mut()
            .build(|cmd: &mut CommandBuffer| {
                Entity::builder()
                    .set(flax::components::name(), name.into())
                    .spawn_into(cmd);
            })
            .boxed()
    }

    fn consume() -> BoxedSystem {
        System::builder()
            .with_query(Query::new(name().added().eq("Foo")))
            .build(|mut q: QueryBorrow<_>| {
                assert_eq!(q.iter().collect_vec(), ["Foo"]);
            })
            .boxed()
    }
    {
        let mut world = World::new();

        let mut schedule = Schedule::builder()
            .with_system(produce("Foo"))
            .flush()
            .with_system(consume())
            .with_system(produce("Bar"))
            .build();

        schedule.execute_seq(&mut world).unwrap();
        assert_eq!(Query::new(name()).borrow(&world).iter().count(), 2);

        schedule.execute_seq(&mut world).unwrap();
        assert_eq!(Query::new(name()).borrow(&world).iter().count(), 4);
    }
}

#[test]
#[cfg(feature = "rayon")]
fn command_flushing_par() {
    fn produce(name: &'static str) -> BoxedSystem {
        System::builder()
            .with_cmd_mut()
            .build(|cmd: &mut CommandBuffer| {
                Entity::builder()
                    .set(flax::components::name(), name.into())
                    .spawn_into(cmd);
            })
            .boxed()
    }

    fn consume() -> BoxedSystem {
        System::builder()
            .with_query(Query::new(name().added().eq("Foo")))
            .build(|mut q: QueryBorrow<_>| {
                assert_eq!(q.iter().collect_vec(), ["Foo"]);
            })
            .boxed()
    }

    {
        let mut world = World::new();

        let mut schedule = Schedule::builder()
            .with_system(produce("Foo"))
            .flush()
            .with_system(consume())
            .with_system(produce("Bar"))
            .build();

        schedule.execute_par(&mut world).unwrap();
        assert_eq!(Query::new(name()).borrow(&world).iter().count(), 2);

        schedule.execute_par(&mut world).unwrap();
        assert_eq!(Query::new(name()).borrow(&world).iter().count(), 4);
    }
}
