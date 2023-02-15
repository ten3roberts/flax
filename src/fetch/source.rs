use core::fmt::Debug;

use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slot},
    entity::EntityLocation,
    ArchetypeId, Entity, Fetch, FetchItem, World,
};

use super::{FetchPrepareData, PreparedFetch, ReadOnlyFetch};

pub trait FetchSource {
    fn resolve(&self, world: &World) -> Option<EntityLocation>;
    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result;
}

impl FetchSource for Entity {
    fn resolve(&self, world: &World) -> Option<EntityLocation> {
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

impl<'q, Q, S> FetchItem<'q> for Source<Q, S>
where
    Q: FetchItem<'q>,
    S: FetchSource,
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

    type Prepared = PreparedSource<Q::Prepared>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let loc = self.source.resolve(data.world)?;

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

    fn access(&self, data: super::FetchAccessData) -> Vec<crate::Access> {
        let loc = self.source.resolve(data.world);

        if let Some(loc) = loc {
            let arch = data.world.archetypes.get(loc.arch_id);
            self.fetch.access(super::FetchAccessData {
                arch_id: loc.arch_id,
                world: data.world,
                arch,
            })
        } else {
            Vec::new()
        }
    }
}

impl<'q, Q> ReadOnlyFetch<'q> for PreparedSource<Q>
where
    Q: ReadOnlyFetch<'q>,
{
    unsafe fn fetch_shared(&'q self, _: crate::archetype::Slot) -> Self::Item {
        self.fetch.fetch_shared(self.slot)
    }
}

impl<'q, Q> PreparedFetch<'q> for PreparedSource<Q>
where
    Q: ReadOnlyFetch<'q>,
{
    type Item = Q::Item;

    unsafe fn fetch(&'q mut self, slot: usize) -> Self::Item {
        self.fetch_shared(slot)
    }

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        self.fetch.filter_slots(slots)
    }
}

pub struct PreparedSource<Q> {
    slot: Slot,
    fetch: Q,
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use crate::{component, entity_ids, name, FetchExt, Query};

    use super::*;

    #[test]
    fn id_source() {
        component! {
            a: u32,
        }
        let mut world = World::new();

        let id1 = Entity::builder()
            .set(name(), "id1".to_string())
            .spawn(&mut world);
        let id2 = Entity::builder()
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
