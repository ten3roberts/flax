use core::{fmt::Debug, marker::PhantomData};

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slice, Slot},
    entity::EntityLocation,
    system::Access,
    ComponentValue, Entity, Fetch, FetchItem, RelationExt, World,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch, ReadOnlyFetch};

pub trait FetchSource {
    fn resolve(&self, arch: &Archetype, world: &World) -> Option<EntityLocation>;
    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result;
}

/// Selects the fetch value from the first parent/object of the specified relation
pub struct FromRelation {
    relation: Entity,
    name: &'static str,
}

impl FromRelation {
    /// Resolves the fetch value from relation
    pub fn new<T: ComponentValue, R: RelationExt<T>>(relation: R) -> Self {
        Self {
            relation: relation.id(),
            name: relation.vtable().name,
        }
    }
}

impl FetchSource for FromRelation {
    fn resolve(&self, arch: &Archetype, world: &World) -> Option<EntityLocation> {
        let (key, _) = arch.relations_like(self.relation).next()?;
        let object = key.object().unwrap();

        Some(
            world
                .location(object)
                .expect("Relation contains invalid entity"),
        )
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl FetchSource for Entity {
    fn resolve(&self, _: &Archetype, world: &World) -> Option<EntityLocation> {
        world.location(*self).ok()
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.fmt(f)
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
    Q::Prepared: for<'x> ReadOnlyFetch<'x>,
    S: FetchSource,
{
    const MUTABLE: bool = Q::MUTABLE;

    type Prepared = PreparedSource<'w, Q::Prepared>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let loc = self.source.resolve(data.arch, data.world)?;

        let arch = data.world.archetypes.get(loc.arch_id);

        // Bounce to the resolved archetype
        let fetch = self.fetch.prepare(FetchPrepareData {
            arch,
            arch_id: loc.arch_id,
            old_tick: data.old_tick,
            new_tick: data.new_tick,
            world: data.world,
        })?;

        Some(PreparedSource {
            slot: loc.slot,
            fetch,
            _marker: PhantomData,
        })
    }

    fn filter_arch(&self, _: &crate::archetype::Archetype) -> bool {
        true
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.fetch.describe(f)?;
        write!(f, "(")?;
        self.source.describe(f)?;
        write!(f, ")")?;
        Ok(())
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        let loc = self.source.resolve(data.arch, data.world);

        if let Some(loc) = loc {
            let arch = data.world.archetypes.get(loc.arch_id);
            self.fetch.access(
                FetchAccessData {
                    arch_id: loc.arch_id,
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
    Q: 'w + ReadOnlyFetch<'q>,
{
    type Item = Q::Item;

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.fetch.filter_slots(slots)
    }

    type Chunk = Q::Chunk;

    unsafe fn create_chunk(&'q mut self, _: crate::archetype::Slice) -> Self::Chunk {
        self.fetch.create_chunk(Slice::single(self.slot))
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk, _: Slot) -> Self::Item {
        Q::fetch_shared_chunk(chunk, 0)
    }
}

pub struct PreparedSource<'w, Q> {
    slot: Slot,
    fetch: Q,
    _marker: PhantomData<&'w mut ()>,
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use crate::{child_of, component, entity_ids, name, FetchExt, Query, Topo};

    use super::*;

    component! {
        a: u32,
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
