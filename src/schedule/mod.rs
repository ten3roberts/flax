use std::collections::BTreeMap;

use itertools::Itertools;

use crate::{system::SystemContext, BoxedSystem, CommandBuffer, World};

/// A collection of systems to run on the world
pub struct Schedule {
    systems: Vec<BoxedSystem>,

    batches: Option<Vec<Vec<usize>>>,

    archetype_gen: u32,
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            batches: None,
            archetype_gen: 0,
        }
    }

    /// Add a new system to the schedule.
    /// Respects order.
    pub fn with_system(&mut self, system: impl Into<BoxedSystem>) -> &mut Self {
        self.batches = None;
        self.systems.push(system.into());
        self
    }

    /// Execute all systems in the schedule sequentially on the world.
    /// Returns the first error and aborts if the execution fails.
    pub fn execute_seq(&mut self, world: &mut World) -> eyre::Result<()> {
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);
        self.systems
            .iter_mut()
            .try_for_each(|system| system.execute(&ctx))?;

        Ok(())
    }

    #[cfg(feature = "parallel")]
    pub fn execute_par(&mut self, world: &mut World) -> eyre::Result<()> {
        use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

        let w_gen = world.archetype_gen();
        // New archetypes
        if self.archetype_gen != w_gen {
            self.batches = None;
            self.archetype_gen = w_gen;
        }

        let systems = &mut self.systems[..];

        let batches = self
            .batches
            .get_or_insert_with(|| Self::build_dependencies(systems, world));

        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);

        let systems = &self.systems;
        let result = batches.iter().for_each(|batch| {
            batch.par_iter().for_each(|&idx| {
                // SAFETY
                // The idx is guaranteed to be disjoint by sort_topo
                let system =
                    unsafe { &mut *(&systems[idx] as *const BoxedSystem as *mut BoxedSystem) };

                todo!()
                // system.execute(&ctx)
            })
        });

        todo!()
    }

    fn build_dependencies(systems: &mut [BoxedSystem], world: &mut World) -> Vec<Vec<usize>> {
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);

        let accesses = systems.iter_mut().map(|v| v.access(&ctx)).collect_vec();

        let mut deps = BTreeMap::new();

        for (dst_idx, dst) in accesses.iter().enumerate() {
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

        dbg!(&deps);

        // Topo sort
        topo_sort(systems, &deps)
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

    dbg!(&visited);
    let groups = result.into_iter().group_by(|v| match visited.get(v) {
        Some(VisitedState::Visited(depth)) => depth,
        _ => unreachable!(),
    });

    let result = groups
        .into_iter()
        .map(|(_, v)| v.collect_vec())
        .collect_vec();

    result
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {

    use itertools::Itertools;

    use crate::{schedule::Schedule, system::System, EntityBuilder, Query, QueryData, World};

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

        let mut schedule = Schedule::new();
        schedule.with_system(system_a).with_system(system_b);

        schedule.execute_seq(&mut world).unwrap();

        world.despawn(id).unwrap();
        let result: eyre::Result<()> = schedule.execute_seq(&mut world).map_err(Into::into);

        eprintln!("{result:?}");
        assert!(result.is_err());
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
