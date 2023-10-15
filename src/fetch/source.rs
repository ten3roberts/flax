use core::{fmt::Debug, marker::PhantomData};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, ArchetypeId, Slice, Slot},
    system::Access,
    Entity, Fetch, FetchItem,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch, RandomFetch};

pub trait FetchSource {
    fn resolve<'a, 'w, Q: Fetch<'w>>(
        &self,
        fetch: &Q,
        data: FetchAccessData<'a>,
    ) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)>;

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result;
}

/// Selects the fetch value from the first parent/object of the specified relation
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
            let object = key.object().unwrap();

            let loc = data
                .world
                .location(object)
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
    slot: Option<Slot>,
) -> Option<(ArchetypeId, &'a Archetype, Option<Slot>)> {
    if fetch.filter_arch(data) {
        return (data.arch_id, data.arch, slot).into();
    }

    for (key, _) in data.arch.relations_like(relation) {
        let object = key.object().unwrap();

        let loc = data
            .world
            .location(object)
            .expect("Relation contains invalid entity");

        let data = FetchAccessData {
            arch_id: loc.arch_id,
            arch: data.world.archetypes.get(loc.arch_id),
            world: data.world,
        };

        if let Some(v) = traverse_resolve(relation, fetch, data, Some(loc.slot)) {
            return Some(v);
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
        return traverse_resolve(self.relation, fetch, data, None);
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
pub struct Source<Q, S> {
    fetch: Q,
    source: S,
}

impl<Q, S> Source<Q, S> {
    /// Creates a new source fetch
    pub fn new(fetch: Q, source: S) -> Self {
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

// impl<'w, 'q, Q> ReadOnlyFetch<'q> for PreparedSource<Q>
// where
//     Q: ReadOnlyFetch<'q>,
// {
//     unsafe fn fetch_shared(&'q self, _: crate::archetype::Slot) -> Self::Item {
//         self.fetch.fetch_shared(self.slot)
//     }
// }

impl<'w, 'q, Q> PreparedFetch<'q> for PreparedSource<'w, Q>
where
    Q: 'w + RandomFetch<'q>,
{
    type Item = Q::Item;

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        if let Some(slot) = self.slot {
            if self.fetch.filter_slots(Slice::single(slot)).is_empty() {
                return Slice::new(slots.end, slots.end);
            } else {
                return slots;
            }
        } else {
            return self.fetch.filter_slots(slots);
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
