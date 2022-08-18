use eyre::WrapErr;
use std::{collections::BTreeMap, mem};

use itertools::Itertools;
use tracing::debug;

use crate::{system::SystemContext, BoxedSystem, CommandBuffer, NeverSystem, System, World, Write};

enum Systems {
    Unbatched(Vec<BoxedSystem>),
    Batched(Vec<Vec<BoxedSystem>>),
}

impl Default for Systems {
    fn default() -> Self {
        Self::Unbatched(Vec::new())
    }
}

impl Systems {
    fn as_unbatched(&mut self) -> &mut Vec<BoxedSystem> {
        match self {
            Systems::Unbatched(v) => v,
            Systems::Batched(v) => {
                let v = mem::take(v);
                *self = Self::Unbatched(v.into_iter().flatten().collect_vec());
                self.as_unbatched()
            }
        }
    }

    fn as_batched(&mut self) -> Option<&mut Vec<Vec<BoxedSystem>>> {
        if let Self::Batched(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Systems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_list();
        match self {
            Self::Unbatched(v) => {
                list.entries(v.iter());
            }
            Self::Batched(v) => {
                list.entries(v.iter().flatten());
            }
        }

        list.finish()
    }
}

fn flush_system() -> BoxedSystem {
    System::builder()
        .with_name("flush")
        .write::<World>()
        .write::<CommandBuffer>()
        .build(|mut world: Write<World>, mut cmd: Write<CommandBuffer>| {
            cmd.apply(&mut world)
                .wrap_err("Failed to flush commandbuffer in schedule\n")
        })
        .boxed()
}

#[derive(Debug, Default)]
/// Incrementally construct a schedule constisting of systems
pub struct ScheduleBuilder {
    systems: Vec<BoxedSystem>,
}

impl ScheduleBuilder {
    /// Creates a new schedule builder
    pub fn new() -> Self {
        Default::default()
    }

    /// Set the ScheduleBuilder's system
    pub fn with_system(&mut self, system: impl Into<BoxedSystem>) -> &mut Self {
        self.systems.push(system.into());
        self
    }

    /// Flush the current state of the commandbuffer into the world.
    /// Is added automatically at the end
    pub fn flush(&mut self) -> &mut Self {
        self.with_system(flush_system())
    }

    /// Build the schedule
    pub fn build(&mut self) -> Schedule {
        Schedule::from_systems(mem::take(&mut self.flush().systems))
    }
}

/// A collection of systems to run on the world
#[derive(Default)]
pub struct Schedule {
    systems: Systems,

    archetype_gen: u32,
}

impl std::fmt::Debug for Schedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Schedule")
            .field("systems", &self.systems)
            .field("archetype_gen", &self.archetype_gen)
            .finish()
    }
}

impl Schedule {
    /// Creates a new schedule builder
    pub fn builder() -> ScheduleBuilder {
        ScheduleBuilder::default()
    }

    /// Creates a new empty schedule, prefer [Self::builder]
    pub fn new() -> Self {
        Default::default()
    }

    /// Creates a schedule from a group of existing systems
    pub fn from_systems(systems: impl Into<Vec<BoxedSystem>>) -> Self {
        Self {
            systems: Systems::Unbatched(systems.into()),
            archetype_gen: 0,
        }
    }

    /// Append one schedule onto another
    pub fn append(&mut self, mut other: Self) {
        self.systems
            .as_unbatched()
            .append(other.systems.as_unbatched())
    }

    /// Add a new system to the schedule.
    /// Respects order.
    pub fn with_system(mut self, system: impl Into<BoxedSystem>) -> Self {
        self.systems.as_unbatched().push(system.into());
        self
    }

    /// Applies the commands inside of the commandbuffer
    pub fn flush(self) -> Self {
        self.with_system(flush_system())
    }

    /// Execute all systems in the schedule sequentially on the world.
    /// Returns the first error and aborts if the execution fails.
    #[tracing::instrument(skip(self, world))]
    pub fn execute_seq(&mut self, world: &mut World) -> eyre::Result<()> {
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);
        self.systems
            .as_unbatched()
            .iter_mut()
            .try_for_each(|system| system.execute(&ctx))?;

        Ok(())
    }

    #[cfg(feature = "parallel")]
    #[tracing::instrument(skip(self, world))]
    /// Parallel version of [Self::execute_seq]
    pub fn execute_par(&mut self, world: &mut World) -> eyre::Result<()> {
        use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

        let w_gen = world.archetype_gen();
        // New archetypes
        if self.archetype_gen != w_gen {
            self.systems.as_unbatched();
            self.archetype_gen = w_gen;
        }

        // let systems = &mut self.systems[..];

        let batches = match &mut self.systems {
            Systems::Unbatched(systems) => {
                let systems = Self::build_dependencies(systems, world);
                self.systems = Systems::Batched(systems);
                self.systems.as_batched().unwrap()
            }
            Systems::Batched(v) => v,
        };

        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);

        let result = batches.iter_mut().try_for_each(|batch| {
            batch
                .par_iter_mut()
                .try_for_each(|system| system.execute(&ctx))
        });

        result
    }

    #[tracing::instrument(skip_all)]
    fn build_dependencies(systems: &mut [BoxedSystem], world: &mut World) -> Vec<Vec<BoxedSystem>> {
        debug!("Building batches");
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);

        let accesses = systems.iter_mut().map(|v| v.access(&ctx)).collect_vec();

        let mut deps = BTreeMap::new();

        for (dst_idx, dst) in accesses.iter().enumerate() {
            {
                for (i, x) in dst.iter().enumerate() {
                    for y in dst.iter().skip(i + 1) {
                        if !x.is_compatible_with(y) {
                            tracing::error!(
                                "System: {:#?} is not compatible with itself",
                                systems[dst_idx]
                            );
                            panic!("Non self-compatible system");
                        }
                    }
                }
            }

            debug!("Generating deps for {dst_idx}");
            let accesses = &accesses;
            let dst_deps = dst
                .iter()
                .flat_map(move |dst| {
                    accesses
                        .iter()
                        .take(dst_idx)
                        .enumerate()
                        .flat_map(|(src_idx, src)| src.iter().map(move |v| (src_idx, v)))
                        .filter(|(_, src)| !src.is_compatible_with(dst))
                        .map(|(src_idx, _)| src_idx)
                })
                .collect_vec();

            deps.insert(dst_idx, dst_deps);
        }

        // Topo sort
        let depths = topo_sort(systems, &deps);

        depths
            .into_iter()
            .map(|depth| {
                depth
                    .into_iter()
                    .map(|idx| mem::replace(&mut systems[idx], BoxedSystem::new(NeverSystem)))
                    .collect_vec()
            })
            .collect_vec()
    }
}

#[derive(Debug, Clone, Copy)]
enum VisitedState {
    Pending,
    Visited(u32),
}

fn topo_sort<T>(items: &[T], deps: &BTreeMap<usize, Vec<usize>>) -> Vec<Vec<usize>> {
    let mut visited = BTreeMap::new();
    let mut result = Vec::new();

    fn inner<T>(
        idx: usize,
        items: &[T],
        deps: &BTreeMap<usize, Vec<usize>>,
        visited: &mut BTreeMap<usize, VisitedState>,
        result: &mut Vec<usize>,
        depth: u32,
    ) {
        match visited.get_mut(&idx) {
            Some(VisitedState::Pending) => panic!("cyclic dependency"),
            Some(VisitedState::Visited(d)) => {
                if depth > *d {
                    // Update self and children
                    *d = depth;
                    deps.get(&idx).into_iter().flatten().for_each(|&dep| {
                        inner(dep, items, deps, visited, result, depth + 1);
                    });
                }
            }
            None => {
                visited.insert(idx, VisitedState::Pending);

                // First, push all dependencies
                deps.get(&idx).into_iter().flatten().for_each(|&dep| {
                    inner(dep, items, deps, visited, result, depth + 1);
                });

                visited.insert(idx, VisitedState::Visited(depth));
                result.push(idx)
            }
        }
    }

    for i in 0..items.len() {
        inner(i, items, deps, &mut visited, &mut result, 0)
    }

    let groups = result.into_iter().group_by(|v| match visited.get(v) {
        Some(VisitedState::Visited(depth)) => depth,
        _ => unreachable!(),
    });

    groups
        .into_iter()
        .map(|(_, v)| v.collect_vec())
        .collect_vec()
}

#[cfg(test)]
mod test {

    use itertools::Itertools;

    use crate::{
        component, schedule::Schedule, system::System, EntityBuilder, Query, QueryData, World,
    };

    use super::topo_sort;

    #[test]
    fn schedule_seq() {
        component! {
            a: String,
            b: i32,
        };

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(a(), "Foo".to_string())
            .set(b(), 5)
            .spawn(&mut world);

        let mut prev_count: i32 = 0;
        let system_a = System::builder()
            .with(Query::new(a()))
            .build(move |mut a: QueryData<_>| {
                let count = a.prepare().iter().count() as i32;

                eprintln!("Change: {prev_count} -> {count}");
                prev_count = count;
            });

        let system_b = System::builder().with(Query::new(b())).build(
            move |mut query: QueryData<_>| -> eyre::Result<()> {
                let mut query = query.prepare();
                let item: &i32 = query.get(id)?;
                eprintln!("Item: {item}");

                Ok(())
            },
        );

        let mut schedule = Schedule::new().with_system(system_a).with_system(system_b);

        schedule.execute_seq(&mut world).unwrap();

        world.despawn(id).unwrap();
        let result: eyre::Result<()> = schedule.execute_seq(&mut world).map_err(Into::into);

        eprintln!("{result:?}");
        eprintln!("Err: {:?}", result.unwrap_err());
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn schedule_par() {
        use crate::{
            components::name, entities, CmpExt, CommandBuffer, Component, EntityFetch, Mutable,
            Write,
        };

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
            pos: (f32, f32),
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
            .set(pos(), (0.0, 0.0))
            .spawn(&mut world);

        builder
            .set(name(), "swordsman".to_string())
            .set(health(), 200.0)
            .set(damage(), 20.0)
            .set(weapon(), Weapon::Sword)
            .set(pos(), (10.0, 1.0))
            .spawn(&mut world);

        builder
            .set(name(), "crossbow_archer".to_string())
            .set(health(), 100.0)
            .set(damage(), 20.0)
            .set(range(), 48.0)
            .set(weapon(), Weapon::Crossbow)
            .set(pos(), (17.0, 20.0))
            .spawn(&mut world);

        builder
            .set(name(), "peasant_1".to_string())
            .set(health(), 100.0)
            .set(pos(), (10.0, 10.0))
            .spawn(&mut world);

        let heal = System::builder()
            .with(Query::new(health().as_mut()))
            .with_name("heal")
            .build(|mut q: QueryData<crate::Mutable<f32>>| {
                q.prepare().iter().for_each(|h| {
                    if *h > 0.0 {
                        *h += 1.0
                    }
                })
            });

        let cleanup = System::builder()
            .with(Query::new(entities()).filter(health().lte(0.0)))
            .write::<CommandBuffer>()
            .with_name("cleanup")
            .build(|mut q: QueryData<_, _>, mut cmd: Write<CommandBuffer>| {
                q.prepare().iter().for_each(|id| {
                    eprintln!("Cleaning up: {id}");
                    cmd.despawn(id);
                })
            });

        let battle =
            System::builder()
                .with(Query::new((entities(), damage(), range(), pos())))
                .with(Query::new((entities(), pos(), health().as_mut())))
                .with_name("battle")
                .build(
                    |mut sub: QueryData<(
                        EntityFetch,
                        Component<f32>,
                        Component<f32>,
                        Component<(f32, f32)>,
                    )>,
                     mut obj: QueryData<(
                        EntityFetch,
                        Component<(f32, f32)>,
                        Mutable<f32>,
                    )>| {
                        // Lock the queries for the whole duration.
                        // There is not much difference in calling `prepare().iter()` for each inner iteration of the loop.
                        let mut sub = sub.prepare();
                        let mut obj = obj.prepare();
                        eprintln!("Prepared queries, commencing battles");
                        for (id1, damage, range, pos) in sub.iter() {
                            for (id2, other_pos, health) in obj.iter() {
                                let rel: (f32, f32) = (other_pos.0 - pos.0, other_pos.1 - pos.1);
                                let dist = (rel.0 * rel.0 + rel.1 * rel.1).sqrt();
                                // We are within range
                                if dist < *range {
                                    eprintln!("{id1} Applying {damage} damage to {id2}");
                                    *health -= damage;
                                }
                            }
                        }
                    },
                );

        let remaining = System::builder()
            .with_name("remaining")
            .with(Query::new(entities()))
            .build(|mut q: QueryData<EntityFetch>| {
                let mut q = q.prepare();
                eprintln!("Remaining: {:?}", q.iter().format(", "));
            });

        let mut schedule = Schedule::new()
            .with_system(heal)
            .with_system(cleanup)
            .flush()
            .with_system(battle)
            .with_system(remaining);

        for _ in 0..32 {
            eprintln!("--------");
            schedule.execute_par(&mut world).unwrap();
        }
    }

    #[test]
    fn test_topo_sort() {
        let items = vec!["a", "b", "c", "d", "e", "f"];
        // a => b c
        // b => c d
        // e => a c
        let deps = [(0, vec![1, 2]), (1, vec![2, 3]), (4, vec![0, 2])].into();

        let sorted = topo_sort(&items, &deps)
            .into_iter()
            .map(|v| v.into_iter().map(|i| items[i]).collect_vec())
            .collect_vec();

        assert_eq!(
            sorted,
            [vec!["c", "d"], vec!["b"], vec!["a"], vec!["e", "f"]]
        )
    }
}
