use eyre::WrapErr;
use std::{collections::BTreeMap, iter::FromIterator, mem, ops::Deref};

use itertools::Itertools;

use crate::{
    access_info, system::SystemContext, AccessInfo, BoxedSystem, CommandBuffer, NeverSystem,
    System, Verbatim, World,
};

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
        .build(|world: &mut World, cmd: &mut CommandBuffer| {
            cmd.apply(world)
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
        Schedule::from_systems(mem::take(&mut self.systems))
    }
}

/// Represents diagnostic information about a system
#[derive(Debug, Clone)]
pub struct SystemInfo {
    name: String,
    desc: Verbatim,
    access: AccessInfo,
}

impl SystemInfo {
    /// Returns a verbose system description
    pub fn desc(&self) -> &str {
        &self.desc.0
    }

    /// Returns the system name
    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    /// Returns the system's current accesses
    pub fn access(&self) -> &AccessInfo {
        &self.access
    }
}

/// A collection of systems to run on the world
#[derive(Default)]
pub struct Schedule {
    systems: Systems,
    cmd: CommandBuffer,

    archetype_gen: u32,
}

/// Holds information regarding a schedules batches
#[derive(Debug, Clone)]
pub struct BatchInfos(Vec<BatchInfo>);
impl Deref for BatchInfos {
    type Target = [BatchInfo];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Holds information regarding a single batch
#[derive(Debug, Clone)]
pub struct BatchInfo(Vec<SystemInfo>);

impl Deref for BatchInfo {
    type Target = [SystemInfo];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Debug for Schedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Schedule")
            .field("systems", &self.systems)
            .field("archetype_gen", &self.archetype_gen)
            .finish()
    }
}

impl FromIterator<BoxedSystem> for Schedule {
    fn from_iter<T: IntoIterator<Item = BoxedSystem>>(iter: T) -> Self {
        Self::from_systems(iter.into_iter().collect_vec())
    }
}

impl<T> From<T> for Schedule
where
    T: IntoIterator<Item = BoxedSystem>,
{
    fn from(v: T) -> Self {
        Self::from_systems(v.into_iter().collect_vec())
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
            cmd: CommandBuffer::new(),
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
    pub fn execute_seq(&mut self, world: &mut World) -> eyre::Result<()> {
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute_seq").entered();

        self.systems
            .as_unbatched()
            .iter_mut()
            .try_for_each(|system| system.execute(&ctx))?;

        self.cmd
            .apply(world)
            .wrap_err("Failed to apply commandbuffer")?;

        Ok(())
    }

    #[cfg(feature = "parallel")]
    /// Parallel version of [Self::execute_seq]
    pub fn execute_par(&mut self, world: &mut World) -> eyre::Result<()> {
        use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute_par").entered();

        self.calculate_batches(world);

        let batches = self.systems.as_batched().unwrap();

        let ctx = SystemContext::new(world, &mut self.cmd);

        let result = batches.iter_mut().try_for_each(|batch| {
            batch
                .par_iter_mut()
                .try_for_each(|system| system.execute(&ctx))
        });

        self.cmd
            .apply(world)
            .wrap_err("Failed to apply commandbuffer")?;

        result
    }

    fn calculate_batches(&mut self, world: &mut World) -> &mut Vec<Vec<BoxedSystem>> {
        let w_gen = world.archetype_gen();
        // New archetypes
        if self.archetype_gen != w_gen {
            self.systems.as_unbatched();
            self.archetype_gen = w_gen;
        }

        match self.systems {
            Systems::Unbatched(ref mut systems) => {
                let systems = Self::build_dependencies(systems, world);
                self.systems = Systems::Batched(systems);
                self.systems.as_batched().unwrap()
            }
            Systems::Batched(ref mut v) => v,
        }
    }

    /// Returns information about the current multithreaded batch partioning and system accesses.
    pub fn batch_info(&mut self, world: &mut World) -> BatchInfos {
        let batches = self
            .calculate_batches(world)
            .iter()
            .map(|batch| {
                let systems = batch
                    .iter()
                    .map(|system| SystemInfo {
                        name: system.name(),
                        desc: Verbatim(format!("{system:#?}")),
                        access: access_info(&system.access(world), world),
                    })
                    .collect_vec();
                BatchInfo(systems)
            })
            .collect_vec();

        BatchInfos(batches)
    }

    fn build_dependencies(systems: &mut [BoxedSystem], world: &mut World) -> Vec<Vec<BoxedSystem>> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("build_dependencies", systems = ?systems).entered();

        let accesses = systems
            .iter_mut()
            .map(|v| (v.name(), v.access(world)))
            .collect_vec();

        let mut deps = BTreeMap::new();

        for (dst_idx, dst) in accesses.iter().enumerate() {
            let accesses = &accesses;
            let dst_deps =
                dst.1
                    .iter()
                    .flat_map(|dst_access| {
                        accesses.iter().take(dst_idx).enumerate().filter_map(
                            move |(src_idx, src)| {
                                if src.1.iter().any(move |v| !v.is_compatible_with(dst_access)) {
                                    Some(src_idx)
                                } else {
                                    None
                                }
                            },
                        )
                    })
                    .dedup()
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

    fn inner(
        idx: usize,
        deps: &BTreeMap<usize, Vec<usize>>,
        visited: &mut BTreeMap<usize, VisitedState>,
        result: &mut Vec<usize>,
        depth: u32,
    ) {
        match visited.get_mut(&idx) {
            Some(VisitedState::Pending) => unreachable!("cyclic dependency"),
            Some(VisitedState::Visited(d)) => {
                if depth > *d {
                    // Update self and children
                    *d = depth;
                    deps.get(&idx).into_iter().flatten().for_each(|&dep| {
                        inner(dep, deps, visited, result, depth + 1);
                    });
                }
            }
            None => {
                visited.insert(idx, VisitedState::Pending);

                // First, push all dependencies
                deps.get(&idx).into_iter().flatten().for_each(|&dep| {
                    inner(dep, deps, visited, result, depth + 1);
                });

                visited.insert(idx, VisitedState::Visited(depth));
                result.push(idx)
            }
        }
    }

    for i in 0..items.len() {
        inner(i, deps, &mut visited, &mut result, 0)
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
        component, schedule::Schedule, system::System, EntityBuilder, Query, QueryBorrow, World,
    };

    use super::topo_sort;

    #[test]
    #[cfg_attr(miri, ignore)]
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
        let system_a = System::builder().with(Query::new(a())).build(
            move |mut a: QueryBorrow<_>| -> eyre::Result<()> {
                let count = a.iter().count() as i32;

                eprintln!("Change: {prev_count} -> {count}");
                prev_count = count;
                Ok(())
            },
        );

        let system_b = System::builder().with(Query::new(b())).build(
            move |mut query: QueryBorrow<_>| -> eyre::Result<()> {
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
    #[cfg_attr(miri, ignore)]
    fn schedule_par() {
        use crate::{
            components::name, entity_ids, CmpExt, CommandBuffer, Component, EntityIds, Mutable,
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
            .build(|mut q: QueryBorrow<crate::Mutable<f32>>| {
                q.iter().for_each(|h| {
                    if *h > 0.0 {
                        *h += 1.0
                    }
                })
            });

        let cleanup = System::builder()
            .with(Query::new(entity_ids()).filter(health().le(0.0)))
            .write::<CommandBuffer>()
            .with_name("cleanup")
            .build(|mut q: QueryBorrow<_, _>, cmd: &mut CommandBuffer| {
                q.iter().for_each(|id| {
                    eprintln!("Cleaning up: {id}");
                    cmd.despawn(id);
                })
            });

        let battle = System::builder()
            .with(Query::new((entity_ids(), damage(), range(), pos())))
            .with(Query::new((entity_ids(), pos(), health().as_mut())))
            .with_name("battle")
            .build(
                |mut sub: QueryBorrow<(_, _, Component<f32>, Component<(f32, f32)>)>,
                 mut obj: QueryBorrow<(_, Component<(f32, f32)>, Mutable<f32>)>| {
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
            .with(Query::new(entity_ids()))
            .build(|mut q: QueryBorrow<EntityIds>| {
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
