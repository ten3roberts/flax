use flax::{
    component, components::name, BoxedSystem, CommandBuffer, Entity, EntityBuilder, FetchExt,
    Query, QueryBorrow, Schedule, System, World,
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

#[test]
#[cfg_attr(miri, ignore)]
fn schedule_seq() {
    component! {
        a: String,
        b: i32,
    };

    let mut world = World::new();

    let id = EntityBuilder::new()
        .set(a(), "Foo".into())
        .set(b(), 5)
        .spawn(&mut world);

    let system_a = System::builder().with_query(Query::new(a())).build(
        move |mut a: QueryBorrow<_>| -> anyhow::Result<()> {
            let _count = a.iter().count() as i32;

            Ok(())
        },
    );

    let system_b = System::builder().with_query(Query::new(b())).build(
        move |mut query: QueryBorrow<_>| -> anyhow::Result<()> {
            let _item: &i32 = query.get(id).map_err(into_anyhow)?;

            Ok(())
        },
    );

    let mut schedule = Schedule::new().with_system(system_a).with_system(system_b);

    schedule.execute_seq(&mut world).unwrap();

    world.despawn(id).unwrap();
    let result: anyhow::Result<()> = schedule.execute_seq(&mut world).map_err(Into::into);

    assert!(result.is_err());
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg(feature = "rayon")]
#[cfg(feature = "std")]
fn schedule_input() {
    component! {
        a: String,
        b: i32,
    };

    let mut world = World::new();

    let id = EntityBuilder::new()
        .set(a(), "Foo".into())
        .set(b(), 5)
        .spawn(&mut world);

    let system_a = System::builder()
        .with_query(Query::new(a()))
        .with_input()
        .build(
            move |mut a: QueryBorrow<_>, cx: &String| -> anyhow::Result<()> {
                let _count = a.iter().count() as i32;

                assert_eq!(cx, "Foo");

                Ok(())
            },
        )
        .boxed();

    let system_b = System::builder()
        .with_query(Query::new(b()))
        .with_input_mut()
        .build(
            move |mut query: QueryBorrow<_>, cx: &mut String| -> anyhow::Result<()> {
                let _item: &i32 = query.get(id)?;

                assert_eq!(cx, "Foo");
                *cx = "Bar".into();
                Ok(())
            },
        )
        .boxed();

    let system_c = System::builder()
        .with_input()
        .build(move |cx: &String| -> anyhow::Result<()> {
            assert_eq!(cx, "Bar");
            Ok(())
        })
        .boxed();

    let mut schedule = Schedule::new()
        .with_system(system_a)
        .with_system(system_b)
        .with_system(system_c);

    let mut cx = String::from("Foo");

    schedule.execute_par_with(&mut world, &mut cx).unwrap();

    assert_eq!(cx, "Bar");
}

#[test]
fn schedule_input_tuple() {
    let system_a = System::builder()
        .with_name("system_a")
        .with_input_mut::<String>()
        .build(|v: &mut String| {
            assert_eq!(v, "Foo");
            v.push_str("Bar");
        });

    let system_b = System::builder()
        .with_name("system_b")
        .with_input_mut::<i32>()
        .build(|v: &mut i32| {
            assert_eq!(*v, 5);
        });

    let system_c = System::builder()
        .with_name("system_c")
        .with_input::<String>()
        .build(|v: &String| {
            assert_eq!(v, "FooBar");
        });

    let mut schedule = Schedule::new()
        .with_system(system_a)
        .with_system(system_b)
        .with_system(system_c);

    let mut world = World::new();

    let mut a = String::from("Foo");
    let mut b = 5;

    assert_eq!(
        schedule.batch_info(&world).to_names(),
        [&["system_a", "system_b"][..], &["system_c"][..]]
    );

    schedule
        .execute_seq_with(&mut world, (&mut a, &mut b))
        .unwrap();
}

#[test]
#[cfg(feature = "rayon")]
#[cfg(feature = "std")]
#[cfg(feature = "derive")]
fn schedule_par() {
    use glam::{vec2, Vec2};

    use flax::{components::name, entity_ids, CommandBuffer, Component, EntityIds, Fetch, Mutable};

    #[derive(Debug, Clone)]
    enum Weapon {
        Sword,
        Bow,
        Crossbow,
    }

    component! {
        health: f32,
        damage: f32,
        range: f32,
        weapon: Weapon,
        pos: Vec2,
    };

    let mut world = World::new();

    let mut builder = EntityBuilder::new();

    // Create different archetypes
    builder
        .set(name(), "archer".to_string())
        .set(health(), 100.0)
        .set(damage(), 15.0)
        .set(range(), 64.0)
        .set(weapon(), Weapon::Bow)
        .set(pos(), vec2(0.0, 0.0))
        .spawn(&mut world);

    builder
        .set(name(), "swordsman".to_string())
        .set(health(), 200.0)
        .set(damage(), 20.0)
        .set(weapon(), Weapon::Sword)
        .set(pos(), vec2(10.0, 1.0))
        .spawn(&mut world);

    builder
        .set(name(), "crossbow_archer".to_string())
        .set(health(), 100.0)
        .set(damage(), 20.0)
        .set(range(), 48.0)
        .set(weapon(), Weapon::Crossbow)
        .set(pos(), vec2(17.0, 20.0))
        .spawn(&mut world);

    builder
        .set(name(), "peasant_1".to_string())
        .set(health(), 100.0)
        .set(pos(), vec2(10.0, 10.0))
        .spawn(&mut world);

    let heal = System::builder()
        .with_query(Query::new(health().as_mut()))
        .with_name("heal")
        .build(|mut q: QueryBorrow<Mutable<f32>>| {
            q.iter().for_each(|h| {
                if *h > 0.0 {
                    *h += 1.0
                }
            })
        });

    let cleanup = System::builder()
        .with_query(Query::new(entity_ids()))
        .with_cmd_mut()
        .with_name("cleanup")
        .build(|mut q: QueryBorrow<_, _>, cmd: &mut CommandBuffer| {
            q.iter().for_each(|id| {
                eprintln!("Cleaning up: {id}");
                cmd.despawn(id);
            })
        });

    #[derive(Fetch, Debug, Clone)]
    struct BattleSubject {
        id: EntityIds,
        damage: Component<f32>,
        range: Component<f32>,
        pos: Component<Vec2>,
    }

    #[derive(Fetch, Debug, Clone)]
    struct BattleObject {
        id: EntityIds,
        pos: Component<Vec2>,
        health: Mutable<f32>,
    }

    let battle = System::builder()
        .with_query(Query::new(BattleSubject {
            id: EntityIds,
            damage: damage(),
            range: range(),
            pos: pos(),
        }))
        .with_query(Query::new(BattleObject {
            id: EntityIds,
            pos: pos(),
            health: health().as_mut(),
        }))
        .with_name("battle")
        .build(
            |mut sub: QueryBorrow<BattleSubject>, mut obj: QueryBorrow<BattleObject>| {
                eprintln!("Prepared queries, commencing battles");
                for a in sub.iter() {
                    for b in obj.iter() {
                        let rel = *b.pos - *a.pos;
                        let dist = rel.length();
                        // We are within range
                        if dist < *a.range {
                            eprintln!("{} Applying {} damage to {}", a.id, a.damage, b.id);
                            *b.health -= a.damage;
                        }
                    }
                }
            },
        );

    let remaining = System::builder()
        .with_name("remaining")
        .with_query(Query::new(entity_ids()))
        .build(|mut q: QueryBorrow<EntityIds>| {
            eprintln!("Remaining: {:?}", q.iter().format(", "));
        });

    let mut schedule = Schedule::new()
        .with_system(heal)
        .with_system(cleanup)
        .flush()
        .with_system(battle)
        .with_system(remaining);

    rayon::ThreadPoolBuilder::new()
        .build()
        .unwrap()
        .install(|| {
            for _ in 0..32 {
                eprintln!("--------");
                schedule.execute_par(&mut world).unwrap();
            }
        });
}

fn into_anyhow(v: flax::Error) -> anyhow::Error {
    #[cfg(not(feature = "std"))]
    return anyhow::Error::msg(v);

    #[cfg(feature = "std")]
    return anyhow::Error::new(v);
}
