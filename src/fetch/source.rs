use core::{fmt::Debug, marker::PhantomData};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, ArchetypeId, Slice, Slot},
    system::Access,
    ArchetypeSearcher, Entity, Fetch, FetchItem,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch, RandomFetch, TransformFetch};

pub trait FetchSource {
    fn resolve<'a, 'w, Q: Fetch<'w>>(
        &self,
        fetch: &Q,
        data: FetchAccessData<'a>,
    ) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)>;

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result;
}

impl FetchSource for () {
    fn resolve<'a, 'w, Q: Fetch<'w>>(
        &self,
        fetch: &Q,
        data: FetchAccessData<'a>,
    ) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)> {
        let mut searcher = ArchetypeSearcher::default();
        fetch.searcher(&mut searcher);

        let mut result = None;
        searcher.find_archetypes(&data.world.archetypes, |arch_id, arch| {
            if arch.is_empty()
                || !fetch.filter_arch(FetchAccessData {
                    world: data.world,
                    arch,
                    arch_id,
                })
            {
                return false;
            }

            debug_assert!(result.is_none());
            result = Some((arch_id, arch, Some(0)));
            true
        });

        result
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("any")
    }
}

/// Selects the fetch value from the first target of the specified relation
pub struct FromRelation {
    pub(crate) relation: Entity,
    pub(crate) name: &'static str,
}

impl FetchSource for FromRelation {
    fn resolve<'a, 'w, Q: Fetch<'w>>(
        &self,
        fetch: &Q,
        data: FetchAccessData<'a>,
    ) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)> {
        for (key, _) in data.arch.relations_like(self.relation) {
            let target = key.target.unwrap();

            let loc = data
                .world
                .location(target)
                .expect("Relation contains invalid entity");

            let arch = data.world.archetypes.get(loc.arch_id);

            if fetch.filter_arch(FetchAccessData {
                arch,
                arch_id: loc.arch_id,
                ..data
            }) {
                return Some((loc.arch_id, arch, Some(loc.slot)));
            }
        }

        None
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl FetchSource for Entity {
    fn resolve<'a, 'w, Q: Fetch<'w>>(
        &self,
        _fetch: &Q,
        data: FetchAccessData<'a>,
    ) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)> {
        let loc = data.world.location(*self).ok()?;

        Some((
            loc.arch_id,
            data.world.archetypes.get(loc.arch_id),
            Some(loc.slot),
        ))
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.fmt(f)
    }
}

/// Traverse the edges of a relation recursively to find the first entity which matches the fetch
pub struct Traverse {
    pub(crate) relation: Entity,
}

fn traverse_resolve<'a, 'w, Q: Fetch<'w>>(
    relation: Entity,
    fetch: &Q,
    data: FetchAccessData<'a>,
) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)> {
    let mut stack = Vec::new();
    stack.push((data.arch_id, None));
    while let Some((arch_id, slot)) = stack.pop() {
        let data = FetchAccessData {
            arch_id,
            arch: data.world.archetypes.get(arch_id),
            world: data.world,
        };

        if fetch.filter_arch(data) {
            return (arch_id, data.arch, slot).into();
        }

        for (key, _) in data.arch.relations_like(relation) {
            let target = key.target.unwrap();

            let loc = data
                .world
                .location(target)
                .expect("Relation contains invalid entity");

            stack.push((loc.arch_id, Some(loc.slot)))
        }
    }

    None
}
impl FetchSource for Traverse {
    #[inline]
    fn resolve<'a, 'w, Q: Fetch<'w>>(
        &self,
        fetch: &Q,
        data: FetchAccessData<'a>,
    ) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)> {
        traverse_resolve(self.relation, fetch, data)
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "transitive({})", self.relation)
    }
}

/// A fetch which proxies the source of the wrapped fetch.
///
/// This allows you to fetch different entities' components in tandem with the current items in a
/// fetch.
///
/// As an explicit source means the same item may be returned for each in the fetch Q must be read
/// only, so that the returned items can safely alias. Additionally, this reduces collateral damage
/// as it forces mutation to be contained to the currently iterated entity (mostly).
///
/// Sources can be nested, but may not yield aliased mutable references:
///
/// ```compile_fail,E0277
/// use flax::{component, FetchExt, Query, World};
/// component! { value: i32, parent(id): (), }
/// let world = World::new();
/// let mut query = Query::new(value().as_mut().relation(parent).relation(parent));
/// let _borrow = query.borrow(&world);
/// ```
pub struct Source<Q, S> {
    fetch: Q,
    source: S,
}

impl<Q, S> Source<Q, S> {
    /// Creates a new source fetch
    pub const fn new(fetch: Q, source: S) -> Self {
        Self { fetch, source }
    }
}

impl<'q, Q, S> FetchItem<'q> for Source<Q, S>
where
    Q: FetchItem<'q>,
{
    type Item = Q::Item;
}

impl<'w, Q, S> Fetch<'w> for Source<Q, S>
where
    Q: Fetch<'w>,
    Q::Prepared: for<'x> RandomFetch<'x>,
    S: FetchSource,
{
    const MUTABLE: bool = Q::MUTABLE;

    type Prepared = PreparedSource<'w, Q::Prepared>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let (arch_id, arch, slot) = self.source.resolve(&self.fetch, data.into())?;

        // Bounce to the resolved archetype
        let fetch = self.fetch.prepare(FetchPrepareData {
            arch,
            arch_id,
            old_tick: data.old_tick,
            new_tick: data.new_tick,
            world: data.world,
        })?;

        Some(PreparedSource {
            slot,
            fetch,
            _marker: PhantomData,
        })
    }

    fn filter_arch(&self, data: FetchAccessData) -> bool {
        self.source.resolve(&self.fetch, data).is_some()
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.fetch.describe(f)?;
        write!(f, "(")?;
        self.source.describe(f)?;
        write!(f, ")")?;
        Ok(())
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        if let Some((arch_id, arch, _)) = self.source.resolve(&self.fetch, data) {
            self.fetch.access(
                FetchAccessData {
                    arch_id,
                    world: data.world,
                    arch,
                },
                dst,
            )
        }
    }
}

impl<'w, 'q, Q> RandomFetch<'q> for PreparedSource<'w, Q>
where
    Q: 'w + RandomFetch<'q>,
{
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
        // Direct access uses an absolute slot in the resolved archetype.
        self.fetch.fetch_shared(self.slot.unwrap_or(slot))
    }

    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
        // Redirected chunks contain only the resolved slot. Otherwise the slot
        // is an offset relative to the slice passed to create_chunk.
        Q::fetch_shared_chunk(&chunk.0, if chunk.1 { 0 } else { slot })
    }
}

impl<'w, 'q, Q> PreparedFetch<'q> for PreparedSource<'w, Q>
where
    Q: 'w + RandomFetch<'q>,
{
    type Item = Q::Item;
    const HAS_FILTER: bool = Q::HAS_FILTER;

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        if let Some(slot) = self.slot {
            if self.fetch.filter_slots(Slice::single(slot)).is_empty() {
                Slice::new(slots.end, slots.end)
            } else {
                slots
            }
        } else {
            self.fetch.filter_slots(slots)
        }
    }

    type Chunk = (Q::Chunk, bool);

    unsafe fn create_chunk(&'q mut self, slice: crate::archetype::Slice) -> Self::Chunk {
        if let Some(slot) = self.slot {
            (self.fetch.create_chunk(Slice::single(slot)), true)
        } else {
            (self.fetch.create_chunk(slice), false)
        }
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        if chunk.1 {
            Q::fetch_shared_chunk(&chunk.0, 0)
        } else {
            Q::fetch_next(&mut chunk.0)
        }
    }
}

pub struct PreparedSource<'w, Q> {
    slot: Option<Slot>,
    fetch: Q,
    _marker: PhantomData<&'w mut ()>,
}

impl<Q, K, S> TransformFetch<K> for Source<Q, S>
where
    Q: TransformFetch<K>,
    S: FetchSource,
{
    type Output = Source<Q::Output, S>;

    fn transform_fetch(self, method: K) -> Self::Output {
        Source {
            fetch: self.fetch.transform_fetch(method),
            source: self.source,
        }
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use crate::{
        component,
        components::{child_of, name},
        entity_ids, FetchExt, Query, Topo, World,
    };

    use super::*;

    component! {
        a: u32,
        relation(id): (),
        module(id): (),
        ship(id): (),
        endpoint: u32,
        extra: (),
    }

    #[test]
    fn nested_relations() {
        let mut world = World::new();
        // Keep the targets in the same archetype to exercise nonzero slots.
        let ships = [10, 20, 30].map(|value| Entity::builder().set(a(), value).spawn(&mut world));
        let modules = ships.map(|target| Entity::builder().set(ship(target), ()).spawn(&mut world));
        let mut expected = Vec::new();
        for (index, target) in modules.into_iter().enumerate() {
            for row in 0..4 {
                let id = (index * 4 + row) as u32;
                let entity = Entity::builder()
                    .set(endpoint(), id)
                    .set(module(target), ())
                    .spawn(&mut world);
                // Split each group across multiple endpoint archetypes.
                if row % 2 == 0 {
                    world.set(entity, extra(), ()).unwrap();
                }
                expected.push((id, Some((ships[index], (index as u32 + 1) * 10))));
            }
        }

        let missing_ship = Entity::builder().spawn(&mut world);
        let missing_component = Entity::builder()
            .set(ship(missing_ship), ())
            .spawn(&mut world);
        for (id, target) in [
            (12, Some(missing_ship)),
            (13, Some(missing_component)),
            (14, None),
        ] {
            let entity = Entity::builder().set(endpoint(), id).spawn(&mut world);
            if let Some(target) = target {
                world.set(entity, module(target), ()).unwrap();
            }
            expected.push((id, None));
        }

        let mut optional = Query::new((
            endpoint().copied(),
            (entity_ids(), a().copied())
                .relation(ship)
                .relation(module)
                .opt(),
        ));
        assert_eq!(
            optional.borrow(&world).iter().sorted().collect_vec(),
            expected
        );

        let mut required = Query::new((
            endpoint().copied(),
            (entity_ids(), a().copied()).relation(ship).relation(module),
        ));
        let present = expected
            .iter()
            .filter_map(|&(id, value)| value.map(|value| (id, value)))
            .collect_vec();
        assert_eq!(
            required.borrow(&world).iter().sorted().collect_vec(),
            present
        );

        // Multiple live results may share the same ship component.
        let mut shared = Query::new((endpoint().copied(), a().relation(ship).relation(module)));
        let mut borrow = shared.borrow(&world);
        let rows = borrow.iter().sorted().collect_vec();
        assert_eq!(rows.len(), 12);
        for group in rows.chunks(4) {
            assert!(group
                .iter()
                .all(|&(_, value)| core::ptr::eq(value, group[0].1)));
        }

        // Filtering inside a redirected source must inspect the ship slot,
        // not the endpoint or module slot.
        let mut filtered = Query::new((
            endpoint().copied(),
            a().gt(15).relation(ship).relation(module).copied(),
        ));
        let matching = present
            .iter()
            .filter_map(|&(id, (_, value))| (value > 15).then_some((id, value)))
            .collect_vec();
        assert_eq!(
            filtered.borrow(&world).iter().sorted().collect_vec(),
            matching
        );

        // A comparison outside the nested sources calls fetch_shared directly.
        let mut filtered = Query::new((
            endpoint().copied(),
            a().relation(ship).relation(module).gt(15).copied(),
        ));
        assert_eq!(
            filtered.borrow(&world).iter().sorted().collect_vec(),
            matching
        );
    }

    fn prepare_data(world: &World, entity: Entity) -> FetchPrepareData<'_> {
        let loc = world.location(entity).unwrap();
        FetchPrepareData {
            world,
            arch: world.archetypes.get(loc.arch_id),
            arch_id: loc.arch_id,
            old_tick: 0,
            new_tick: 1,
        }
    }

    #[test]
    fn random_source_slots_and_chunks() {
        unsafe fn check_chunk<'q, P: RandomFetch<'q, Item = &'q u32>>(
            prepared: &'q mut P,
            expected: &[u32],
        ) {
            let mut chunk = prepared.create_chunk(Slice::new(1, 4));
            assert_eq!(*P::fetch_shared_chunk(&chunk, 2), expected[2]);
            // Shared random reads do not advance the sequential cursor.
            for value in expected {
                assert_eq!(*P::fetch_next(&mut chunk), *value);
            }
        }

        let mut world = World::new();
        let roots =
            [10, 20, 30, 40, 50].map(|value| Entity::builder().set(a(), value).spawn(&mut world));
        let children = [0; 4].map(|_| {
            Entity::builder()
                .set(relation(roots[3]), ())
                .spawn(&mut world)
        });
        let fetch = a().traverse(relation);

        // A traversal matching the current archetype passes slots through.
        let mut prepared = fetch.prepare(prepare_data(&world, roots[0])).unwrap();
        assert_eq!(prepared.slot, None);
        unsafe {
            assert_eq!(*prepared.fetch_shared(3), 40);
            check_chunk(&mut prepared, &[20, 30, 40]);
        }

        // A redirected traversal ignores the input slot and chunk offset.
        let mut prepared = fetch.prepare(prepare_data(&world, children[0])).unwrap();
        assert_eq!(prepared.slot, Some(3));
        unsafe {
            assert_eq!(*prepared.fetch_shared(1), 40);
            check_chunk(&mut prepared, &[40, 40, 40]);
        }

        // Nesting a passthrough source under a redirect still preserves the
        // target's nonzero slot, including during filtering.
        let mut query = Query::new((
            entity_ids(),
            a().traverse(relation).relation(relation).gt(35).copied(),
        ));
        assert_eq!(
            query.borrow(&world).iter().sorted().collect_vec(),
            children
                .into_iter()
                .map(|id| (id, 40))
                .sorted()
                .collect_vec()
        );
    }

    #[test]
    fn nested_source_access() {
        use crate::system::AccessKind;

        let mut world = World::new();
        let target = Entity::builder().set(a(), 42).spawn(&mut world);
        let middle = Entity::builder().set(ship(target), ()).spawn(&mut world);
        let source = Entity::builder().set(module(middle), ()).spawn(&mut world);
        let data = prepare_data(&world, source);
        let mut accesses = Vec::new();
        a().relation(ship)
            .relation(module)
            .access(data.into(), &mut accesses);
        assert_eq!(
            accesses,
            [Access {
                kind: AccessKind::Archetype {
                    id: world.location(target).unwrap().arch_id,
                    component: a().key(),
                },
                mutable: false,
            }]
        );
    }

    #[test]
    fn parent_fetch() {
        let mut world = World::new();

        let child_1 = Entity::builder()
            .set(name(), "child.1".into())
            .set(a(), 8)
            .spawn(&mut world);

        let root = Entity::builder()
            .set(name(), "root".into())
            .set(a(), 4)
            .spawn(&mut world);

        let child_1_1 = Entity::builder()
            .set(name(), "child.1.1".into())
            .spawn(&mut world);

        let child_2 = Entity::builder()
            .set(name(), "child.2".into())
            .spawn(&mut world);

        world.set(child_1, child_of(root), ()).unwrap();
        world.set(child_2, child_of(root), ()).unwrap();
        world.set(child_1_1, child_of(child_1), ()).unwrap();

        let mut query = Query::new((
            name().deref(),
            (name().deref(), a().copied()).relation(child_of).opt(),
        ))
        .with_strategy(Topo::new(child_of));

        pretty_assertions::assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [
                ("root", None),
                ("child.1", Some(("root", 4))),
                ("child.1.1", Some(("child.1", 8))),
                ("child.2", Some(("root", 4))),
            ]
        );
    }

    #[test]
    fn multi_parent_fetch() {
        let mut world = World::new();

        let child = Entity::builder()
            .set(name(), "child".into())
            .set(a(), 8)
            .spawn(&mut world);

        let parent = Entity::builder()
            .set(name(), "parent".into())
            .spawn(&mut world);

        let parent2 = Entity::builder()
            .set(name(), "parent2".into())
            .set(a(), 8)
            .spawn(&mut world);

        world.set(child, relation(parent), ()).unwrap();
        world.set(child, relation(parent2), ()).unwrap();

        let mut query = Query::new((
            name().deref(),
            (name().deref(), a().copied()).relation(relation).opt(),
        ))
        .with_strategy(Topo::new(relation));

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [
                ("parent", None),
                ("parent2", None),
                ("child", Some(("parent2", 8))),
            ]
        );
    }

    #[test]
    fn traverse() {
        let mut world = World::new();

        let root = Entity::builder()
            .set(name(), "root".into())
            .set(a(), 5)
            .spawn(&mut world);

        let root3 = Entity::builder()
            .set(name(), "root".into())
            .spawn(&mut world);

        let root2 = Entity::builder()
            .set(name(), "root2".into())
            .set(a(), 7)
            .spawn(&mut world);

        let child_1 = Entity::builder()
            .set(name(), "child_1".into())
            .set(relation(root), ())
            .spawn(&mut world);

        let _child_3 = Entity::builder()
            .set(name(), "child_3".into())
            .set(relation(root2), ())
            .spawn(&mut world);

        let _child_4 = Entity::builder()
            .set(name(), "child_4".into())
            .set(relation(root3), ())
            .spawn(&mut world);

        let _child_5 = Entity::builder()
            .set(name(), "child_5".into())
            .set(relation(root3), ())
            .set(relation(root2), ())
            .spawn(&mut world);

        let _child_2 = Entity::builder()
            .set(name(), "child_2".into())
            .set(relation(root), ())
            .spawn(&mut world);

        let _child_1_1 = Entity::builder()
            .set(name(), "child_1_1".into())
            .set(relation(child_1), ())
            .spawn(&mut world);

        let mut query = Query::new((
            name().deref(),
            (name().deref(), a().copied()).traverse(relation),
        ));

        assert_eq!(
            query.borrow(&world).iter().sorted().collect_vec(),
            [
                ("child_1", ("root", 5)),
                ("child_1_1", ("root", 5)),
                ("child_2", ("root", 5)),
                ("child_3", ("root2", 7)),
                ("child_5", ("root2", 7)),
                ("root", ("root", 5)),
                ("root2", ("root2", 7)),
            ]
        );
    }

    #[test]
    fn id_source() {
        let mut world = World::new();

        let _id1 = Entity::builder()
            .set(name(), "id1".to_string())
            .spawn(&mut world);
        let _id2 = Entity::builder()
            .set(name(), "id2".to_string())
            .spawn(&mut world);

        let id3 = Entity::builder()
            .set(name(), "id3".to_string())
            .set(a(), 5)
            .spawn(&mut world);

        let mut query = Query::new((
            name().cloned(),
            Source {
                source: id3,
                fetch: (entity_ids(), a(), name().cloned()),
            },
        ));

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            &[
                ("id1".to_string(), (id3, &5, "id3".to_string())),
                ("id2".to_string(), (id3, &5, "id3".to_string())),
                ("id3".to_string(), (id3, &5, "id3".to_string()))
            ]
        );

        let mut query2 = Query::new((
            name().cloned(),
            Source {
                source: id3,
                fetch: (a().maybe_mut()),
            },
        ));

        for (name, id3_a) in &mut query2.borrow(&world) {
            *id3_a.write() += name.len() as u32;
        }

        use alloc::string::ToString;

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            &[
                ("id1".to_string(), (id3, &14, "id3".to_string())),
                ("id2".to_string(), (id3, &14, "id3".to_string())),
                ("id3".to_string(), (id3, &14, "id3".to_string()))
            ]
        );
    }
}
