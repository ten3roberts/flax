use alloc::vec::Vec;

use crate::{
    archetype::{Archetype, Slot},
    system::{Access, AccessKind},
    EntityRef, Fetch, FetchItem, World,
};

use super::{FetchAccessData, PreparedFetch};

/// Access all components dynamically in a query
pub struct EntityRefs;

/// Access all components dynamically in a query
pub fn entity_refs() -> EntityRefs {
    EntityRefs
}

impl<'q> FetchItem<'q> for EntityRefs {
    type Item = EntityRef<'q>;
}

impl<'w> Fetch<'w> for EntityRefs {
    ///  False since just having an `EntityRef` does not cause any mutation.
    ///
    ///  Mutation through `get_mut` will cause an external change event
    const MUTABLE: bool = false;

    type Prepared = PreparedEntityRef<'w>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedEntityRef {
            arch: data.arch,
            world: data.world,
        })
    }

    fn filter_arch(&self, _: &crate::archetype::Archetype) -> bool {
        true
    }

    fn access(&self, _: FetchAccessData, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::World {},
            mutable: true,
        })
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "entity_ref")
    }

    fn searcher(&self, _: &mut crate::ArchetypeSearcher) {}
}

#[doc(hidden)]
pub struct PreparedEntityRef<'a> {
    world: &'a World,
    arch: &'a Archetype,
}

pub struct Batch<'a> {
    pub(crate) world: &'a World,
    pub(crate) arch: &'a Archetype,
    pub(crate) slot: Slot,
}

impl<'w, 'q> PreparedFetch<'q> for PreparedEntityRef<'w> {
    type Item = EntityRef<'q>;
    type Chunk = Batch<'q>;

    unsafe fn create_chunk(&'q mut self, slots: crate::archetype::Slice) -> Self::Chunk {
        Batch {
            world: self.world,
            arch: self.arch,
            slot: slots.start,
        }
    }

    #[inline]
    unsafe fn fetch_next(batch: &mut Self::Chunk) -> Self::Item {
        let slot = batch.slot;

        EntityRef {
            arch: batch.arch,
            world: batch.world,
            slot,
            id: batch.arch.entities[slot],
        }
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use crate::{component, name, Entity, EntityIds, FetchExt, Query, World};

    #[test]
    fn entity_refs() {
        component! {
            health: f32,
            mortal: (),
        };

        let mut world = World::new();
        let _ = Entity::builder().set(name(), "a".into()).spawn(&mut world);
        let b = Entity::builder()
            .set(name(), "b".into())
            .set(health(), 50.0)
            .spawn(&mut world);

        let c = Entity::builder()
            .set(name(), "c".into())
            .set(health(), 100.0)
            .set(mortal(), ())
            .spawn(&mut world);

        let _ = Entity::builder().set(name(), "d".into()).spawn(&mut world);

        let mut health_changed =
            Query::new((EntityIds, health().copied())).filter(health().modified());

        assert_eq!(
            health_changed.borrow(&world).iter().collect_vec(),
            [(b, 50.0), (c, 100.0)]
        );

        let mut query = Query::new(super::EntityRefs);

        for entity in &mut query.borrow(&world) {
            if entity.has(mortal()) {
                if let Ok(mut health) = entity.get_mut(health()) {
                    *health *= 0.5;
                }
            }
        }

        assert_eq!(
            health_changed.borrow(&world).iter().collect_vec(),
            [(c, 50.0)]
        );

        assert_eq!(health_changed.borrow(&world).iter().collect_vec(), []);
    }
}
