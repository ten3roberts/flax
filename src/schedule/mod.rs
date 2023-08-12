use core::{marker::PhantomData, mem, ops::Deref};

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use anyhow::Context;
use itertools::Itertools;

use crate::{
    system::{access_info, AccessInfo, SystemContext},
    util::Verbatim,
    BoxedSystem, CommandBuffer, System, World,
};

fn flush_system<T: 'static + Send + Sync>() -> BoxedSystem<T> {
    System::builder_with_data()
        .with_name("flush")
        .with_world_mut()
        .with_cmd_mut()
        .build(|world: &mut World, cmd: &mut CommandBuffer| {
            cmd.apply(world)
                .context("Failed to flush commandbuffer in schedule\n")
        })
        .boxed()
}

#[derive(Debug)]
/// Incrementally construct a schedule constisting of systems
pub struct ScheduleBuilder<T = ()> {
    systems: Vec<BoxedSystem<T>>,
}

impl<T> Default for ScheduleBuilder<T> {
    fn default() -> Self {
        Self {
            systems: Default::default(),
        }
    }
}

impl<T: 'static + Send + Sync> ScheduleBuilder<T> {
    /// Creates a new schedule builder
    pub fn new() -> Self {
        Default::default()
    }

    /// Set the ScheduleBuilder's system
    pub fn with_system(&mut self, system: impl Into<BoxedSystem<T>>) -> &mut Self {
        self.systems.push(system.into());
        self
    }

    /// Flush the current state of the commandbuffer into the world.
    /// Is added automatically at the end
    pub fn flush(&mut self) -> &mut Self {
        self.with_system(flush_system())
    }

    /// Build the schedule
    pub fn build(&mut self) -> Schedule<T> {
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
pub struct Schedule<T = ()> {
    systems: Vec<Vec<BoxedSystem<T>>>,
    cmd: CommandBuffer,

    archetype_gen: u32,
    data: PhantomData<T>,
}

impl<T> Default for Schedule<T> {
    fn default() -> Self {
        Self {
            systems: Default::default(),
            cmd: Default::default(),
            archetype_gen: Default::default(),
            data: Default::default(),
        }
    }
}

/// Holds information regarding a schedule's batches
#[derive(Debug, Clone)]
pub struct BatchInfos(Vec<BatchInfo>);

impl BatchInfos {
    /// Converts the batches into just a list of system names
    pub fn to_names(&self) -> Vec<Vec<String>> {
        self.iter()
            .map(|v| v.iter().map(|v| v.name().into()).collect_vec())
            .collect_vec()
    }
}

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

impl<T> core::fmt::Debug for Schedule<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Schedule")
            .field("systems", &self.systems)
            .field("archetype_gen", &self.archetype_gen)
            .finish()
    }
}

impl<T: 'static + Send + Sync> FromIterator<BoxedSystem<T>> for Schedule<T> {
    fn from_iter<I: IntoIterator<Item = BoxedSystem<T>>>(iter: I) -> Self {
        Self::from_systems(iter.into_iter().collect_vec())
    }
}

impl<T: 'static + Send + Sync, U> From<U> for Schedule<T>
where
    U: IntoIterator<Item = BoxedSystem<T>>,
{
    fn from(v: U) -> Self {
        Self::from_systems(v.into_iter().collect_vec())
    }
}

impl<T: 'static + Send + Sync> Schedule<T> {
    /// Creates a new schedule builder with a custom data type
    pub fn builder_with_data() -> ScheduleBuilder<T> {
        ScheduleBuilder::default()
    }
    /// Creates a new empty schedule, prefer [Self::builder]
    pub fn new() -> Self {
        Default::default()
    }

    /// Creates a schedule from a group of existing systems
    pub fn from_systems(systems: impl Into<Vec<BoxedSystem<T>>>) -> Self {
        Self {
            systems: alloc::vec![systems.into()],
            archetype_gen: 0,
            cmd: CommandBuffer::new(),
            data: PhantomData,
        }
    }

    /// Append one schedule onto another
    pub fn append(&mut self, other: Self) {
        self.archetype_gen = 0;
        self.systems.extend(other.systems)
    }

    /// Add a new system to the schedule.
    /// Respects order.
    pub fn with_system(mut self, system: impl Into<BoxedSystem<T>>) -> Self {
        self.archetype_gen = 0;
        let v = match self.systems.first_mut() {
            Some(v) => v,
            None => {
                self.systems.push(Default::default());
                &mut self.systems[0]
            }
        };

        v.push(system.into());
        self
    }

    /// Applies the commands inside of the commandbuffer
    pub fn flush(self) -> Self {
        self.with_system(flush_system())
    }

    /// Returns information about the current multithreaded batch partioning and system accesses.
    pub fn batch_info(&mut self, world: &World) -> BatchInfos {
        self.systems = Self::build_dependencies(mem::take(&mut self.systems), world);

        let batches = self
            .systems
            .iter()
            .map(|batch| {
                let systems = batch
                    .iter()
                    .map(|system| {
                        let mut access = Vec::new();
                        system.access(world, &mut access);
                        SystemInfo {
                            name: system.name().into(),
                            desc: Verbatim(alloc::format!("{system:#?}")),
                            access: access_info(&access, world),
                        }
                    })
                    .collect_vec();
                BatchInfo(systems)
            })
            .collect_vec();

        BatchInfos(batches)
    }

    /// Same as [`Self::execute_seq`] but allows supplying short lived input available to the systems
    ///
    /// **Note**:
    /// Due to current limitations in Rust, T has to be as `Fn(T): 'static` implies `T: 'static`.
    ///
    /// See:
    /// - <https://github.com/rust-lang/rust/issues/57325>
    /// - <https://stackoverflow.com/questions/53966598/how-to-fnt-static-register-as-static-for-any-generic-type-argument-t>
    pub fn execute_seq_with(&mut self, world: &mut World, input: &mut T) -> anyhow::Result<()> {
        let ctx = SystemContext::new(world, &mut self.cmd, input);

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute_seq").entered();

        self.systems
            .iter_mut()
            .flatten()
            .try_for_each(|system| system.execute(&ctx))?;

        self.cmd
            .apply(world)
            .context("Failed to apply commandbuffer")?;

        Ok(())
    }

    #[cfg(feature = "parallel")]
    /// Same as [`Self::execute_par`] but allows supplying short lived data available to the systems
    pub fn execute_par_with(&mut self, world: &mut World, input: &mut T) -> anyhow::Result<()> {
        use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute_par").entered();

        let w_gen = world.archetype_gen();
        // New archetypes
        if self.archetype_gen != w_gen {
            self.archetype_gen = w_gen;
            self.systems = Self::build_dependencies(mem::take(&mut self.systems), world);
        }

        let ctx = SystemContext::new(world, &mut self.cmd, input);

        let result = self.systems.iter_mut().try_for_each(|batch| {
            batch
                .par_iter_mut()
                .try_for_each(|system| system.execute(&ctx))
        });

        self.cmd
            .apply(world)
            .context("Failed to apply commandbuffer")?;

        result
    }

    fn build_dependencies(
        systems: Vec<Vec<BoxedSystem<T>>>,
        world: &World,
    ) -> Vec<Vec<BoxedSystem<T>>> {
        let accesses = systems
            .iter()
            .flatten()
            .map(|v| {
                let mut access = Vec::new();
                v.access(world, &mut access);
                access
            })
            .collect_vec();

        let mut deps = BTreeMap::new();

        for (dst_idx, dst) in accesses.iter().enumerate() {
            let accesses = &accesses;
            let dst_deps =
                dst.iter()
                    .flat_map(|dst_access| {
                        accesses.iter().take(dst_idx).enumerate().filter_map(
                            move |(src_idx, src)| {
                                if src.iter().any(move |v| !v.is_compatible_with(dst_access)) {
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

        // let mut current_access = BTreeMap::new();
        // let mut batches = Vec::new();

        // let mut current_batch = Vec::new();
        // for (system, (name, accesses)) in systems.into_iter().zip(accesses) {
        //     if !update_compatible(&accesses, &mut current_access) {
        //         eprintln!("Pushing new batch with");
        //         batches.push(mem::take(&mut current_batch));
        //     }
        //     current_batch.push(system);
        //     eprintln!("system: {name}");
        // }

        // batches.push(current_batch);

        // batches

        topo_sort(systems, &deps)
    }
}

impl Schedule<()> {
    /// Creates a new schedule builder
    pub fn builder() -> ScheduleBuilder {
        ScheduleBuilder::default()
    }
    /// Execute all systems in the schedule sequentially on the world.
    /// Returns the first error and aborts if the execution fails.
    pub fn execute_seq(&mut self, world: &mut World) -> anyhow::Result<()> {
        self.execute_seq_with(world, &mut ())
    }

    #[cfg(feature = "parallel")]
    /// Executes the systems in the schedule in parallel.
    ///
    /// Systems will run in an order such that changes and mutable accesses made by systems
    /// provided
    /// *before* are observable when the system runs.
    ///
    /// Systems with no dependencies between each other may run in any order, but will **not** run
    /// before any previously provided system which it depends on.
    ///
    /// A dependency between two systems is given by a side effect, e.g; a component write, which
    /// is accessed by the seconds system through a read or other side effect.
    pub fn execute_par(&mut self, world: &mut World) -> anyhow::Result<()> {
        self.execute_par_with(world, &mut ())
    }
}

///// Insert accesses checking for compatibility.
/////
///// If the new system's accesses are not compatible, the current acceses are replaced with the new
///// system, and false is returned
// fn update_compatible(accesses: &[Access], current: &mut BTreeMap<AccessKind, bool>) -> bool {
//     let compatible = true;
//     for access in accesses {
//         match current.entry(access.kind) {
//             // Add the access as is
//             btree_map::Entry::Vacant(slot) => {
//                 slot.insert(access.mutable);
//             }
//             btree_map::Entry::Occupied(slot) => {
//                 let current_access = *slot.get();

//                 // Compatible iff both are immutable accesses
//                 if access.mutable || current_access {
//                     current.clear();
//                     current.extend(accesses.iter().map(|v| (v.kind, v.mutable)));
//                     return false;
//                 }
//             }
//         }
//     }

//     true
// }

#[derive(Debug, Clone, Copy)]
enum VisitedState {
    Pending,
    Visited(u32),
}

fn topo_sort<T>(items: Vec<Vec<T>>, deps: &BTreeMap<usize, Vec<usize>>) -> Vec<Vec<T>> {
    let mut visited = BTreeMap::new();
    let mut result = Vec::new();

    fn inner(
        idx: usize,
        deps: &BTreeMap<usize, Vec<usize>>,
        visited: &mut BTreeMap<usize, VisitedState>,
        result: &mut Vec<usize>,
    ) -> u32 {
        match visited.get_mut(&idx) {
            Some(VisitedState::Pending) => unreachable!("cyclic dependency"),
            Some(VisitedState::Visited(d)) => *d,
            None => {
                visited.insert(idx, VisitedState::Pending);

                // First, push all dependencies
                // Find the longest path to a root
                let depth = deps
                    .get(&idx)
                    .into_iter()
                    .flatten()
                    .map(|&dep| inner(dep, deps, visited, result))
                    .max()
                    .map(|v| v + 1)
                    .unwrap_or(0);

                visited.insert(idx, VisitedState::Visited(depth));
                result.push(idx);
                depth
            }
        }
    }

    // The property of a graph ensures that for any depth `n` there exists a depth `n-1` where `n
    // > 1` as each non-root node has at least *one* ancestor with depth `n-1`

    let mut batches = Vec::new();
    for (idx, system) in items.into_iter().flatten().enumerate() {
        let depth = inner(idx, deps, &mut visited, &mut result) as usize;
        if depth >= batches.len() {
            batches.resize_with(depth + 1, Vec::default);
        }

        batches[depth].push(system);
    }

    batches
}

#[cfg(test)]
mod test {

    use alloc::{string::String, vec};

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
                let _item: &i32 = query.get(id).map_err(|v| v.into_anyhow())?;

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
    #[cfg(feature = "parallel")]
    #[cfg(feature = "std")]
    fn schedule_context() {
        component! {
            a: String,
            b: i32,
        };

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(a(), "Foo".into())
            .set(b(), 5)
            .spawn(&mut world);

        let system_a = System::builder_with_data()
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

        let system_b = System::builder_with_data()
            .with_query(Query::new(b()))
            .with_input_mut()
            .build(
                move |mut query: QueryBorrow<_>, cx: &mut String| -> anyhow::Result<()> {
                    let _item: &i32 = query.get(id).map_err(|v| v.into_anyhow())?;

                    assert_eq!(cx, "Foo");
                    *cx = "Bar".into();
                    Ok(())
                },
            )
            .boxed();

        let system_c = System::builder_with_data()
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
    #[cfg(feature = "parallel")]
    #[cfg(feature = "std")]
    #[cfg(feature = "derive")]
    fn schedule_par() {
        use glam::{vec2, Vec2};
        use itertools::Itertools;

        use crate::{
            components::name, entity_ids, CommandBuffer, Component, EntityIds, Fetch, Mutable,
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
            .build(|mut q: QueryBorrow<crate::Mutable<f32>>| {
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

    #[test]
    fn test_topo_sort() {
        let items = vec![vec!["a", "b", "c", "d", "e", "f"]];
        // a => b c
        // b => c d
        // e => a c
        let deps = [(0, vec![1, 2]), (1, vec![2, 3]), (4, vec![0, 2])].into();

        let sorted = topo_sort(items, &deps);

        assert_eq!(
            sorted,
            [vec!["c", "d", "f"], vec!["b"], vec!["a"], vec!["e"]]
        )
    }
}
