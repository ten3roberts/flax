use core::{mem, ops::Deref};

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use anyhow::Context;
use itertools::Itertools;

use crate::{
    system::{access_info, AccessInfo, IntoInput, SystemContext},
    util::Verbatim,
    BoxedSystem, CommandBuffer, System, World,
};

fn flush_system() -> BoxedSystem {
    System::builder()
        .with_name("flush")
        .with_world_mut()
        .with_cmd_mut()
        .build(|world: &mut World, cmd: &mut CommandBuffer| {
            profile_scope!("flush");
            cmd.apply(world)
                .context("Failed to flush commandbuffer in schedule\n")
        })
        .boxed()
}

#[derive(Default, Debug)]
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

/// A schedule of systems to execute with automatic parallelization.
#[derive(Default)]
pub struct Schedule {
    systems: Vec<Vec<BoxedSystem>>,
    cmd: CommandBuffer,

    archetype_gen: u32,
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

impl core::fmt::Debug for Schedule {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Schedule")
            .field("systems", &self.systems)
            .field("archetype_gen", &self.archetype_gen)
            .finish()
    }
}

impl FromIterator<BoxedSystem> for Schedule {
    fn from_iter<I: IntoIterator<Item = BoxedSystem>>(iter: I) -> Self {
        Self::from_systems(iter.into_iter().collect_vec())
    }
}

impl<U> From<U> for Schedule
where
    U: IntoIterator<Item = BoxedSystem>,
{
    fn from(v: U) -> Self {
        Self::from_systems(v.into_iter().collect_vec())
    }
}

impl Schedule {
    /// Creates a new empty schedule, prefer [Self::builder]
    pub fn new() -> Self {
        Default::default()
    }

    /// Creates a new schedule builder
    pub fn builder() -> ScheduleBuilder {
        ScheduleBuilder::default()
    }

    /// Execute all systems in the schedule sequentially on the world.
    /// Returns the first error and aborts if the execution fails.
    pub fn execute_seq(&mut self, world: &mut World) -> anyhow::Result<()> {
        self.execute_seq_with(world, &mut ())
    }

    #[cfg(feature = "rayon")]
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

    /// Creates a schedule from a group of existing systems
    pub fn from_systems(systems: impl Into<Vec<BoxedSystem>>) -> Self {
        Self {
            systems: alloc::vec![systems.into()],
            archetype_gen: 0,
            cmd: CommandBuffer::new(),
        }
    }

    /// Append one schedule onto another
    pub fn append(&mut self, other: Self) {
        self.archetype_gen = 0;
        self.systems.extend(other.systems)
    }

    /// Add a new system to the schedule.
    /// Respects order.
    pub fn with_system(mut self, system: impl Into<BoxedSystem>) -> Self {
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

    /// Add a new system to the schedule.
    /// Respects order.
    pub fn add_system(&mut self, system: impl Into<BoxedSystem>) -> &mut Self {
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
    /// The data can be a mutable reference type, or a tuple of mutable references
    pub fn execute_seq_with<'a>(
        &'a mut self,
        world: &'a mut World,
        input: impl IntoInput<'a>,
    ) -> anyhow::Result<()> {
        profile_function!();
        let input = input.into_input();
        let ctx = SystemContext::new(world, &mut self.cmd, &input);

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute_seq").entered();

        for system in self.systems.iter_mut().flatten() {
            system.execute(&ctx)?
        }

        self.cmd
            .apply(world)
            .context("Failed to apply commandbuffer")
    }

    #[cfg(feature = "rayon")]
    /// Same as [`Self::execute_par`] but allows supplying short lived data available to the systems
    pub fn execute_par_with<'a>(
        &'a mut self,
        world: &'a mut World,
        input: impl IntoInput<'a>,
    ) -> anyhow::Result<()> {
        profile_function!();
        use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute_par").entered();

        let w_gen = world.archetype_gen();
        // New archetypes
        if self.archetype_gen != w_gen {
            self.archetype_gen = w_gen;
            self.systems = Self::build_dependencies(mem::take(&mut self.systems), world);
        }

        let input = input.into_input();
        let mut ctx = SystemContext::new(world, &mut self.cmd, &input);

        let mut batches = self.systems.iter_mut();

        for batch in &mut batches {
            batch
                .par_iter_mut()
                .try_for_each(|system| system.execute(&ctx))?;

            // If the archetype generation changed the batches are invalidated
            //
            // Execute sequentially, and rebuild the schedule next time around
            if self.archetype_gen != ctx.world.get_mut().archetype_gen() {
                return Self::bail_seq(batches, &mut ctx);
            }
        }

        self.cmd
            .apply(world)
            .context("Failed to apply commandbuffer")
    }

    #[cfg(feature = "rayon")]
    fn bail_seq(
        batches: core::slice::IterMut<Vec<BoxedSystem>>,
        ctx: &mut SystemContext<'_, '_, '_>,
    ) -> anyhow::Result<()> {
        for system in batches.flatten() {
            system.execute(ctx)?;
        }

        ctx.cmd
            .get_mut()
            .apply(ctx.world.get_mut())
            .context("Failed to apply commandbuffer")
    }

    fn build_dependencies(systems: Vec<Vec<BoxedSystem>>, world: &World) -> Vec<Vec<BoxedSystem>> {
        profile_function!();
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

    use alloc::vec;

    use super::topo_sort;

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
