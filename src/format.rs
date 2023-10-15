use core::fmt::{self, Debug, Formatter};

use crate::{
    archetype::{Archetype, Slot},
    component::ComponentKey,
    metadata::debuggable,
    Entity, Fetch, Query, World,
};

/// Debug formats the world with the given filter.
/// Created using [World::format_debug]
pub struct WorldFormatter<'a, F> {
    pub(crate) world: &'a World,
    pub(crate) filter: F,
}

impl<'a, F> fmt::Debug for WorldFormatter<'a, F>
where
    F: for<'x> Fetch<'x>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_map();

        let mut query = Query::new(())
            .with_components()
            .filter(self.filter.by_ref());

        let mut query = query.borrow(self.world);

        for batch in query.iter_batched() {
            let arch = batch.arch();
            for slot in batch.slots().iter() {
                assert!(
                    slot < arch.len(),
                    "batch is larger than archetype, chunk: {:?}, arch: {:?}",
                    batch.slots(),
                    arch.entities()
                );

                let row = RowValueFormatter {
                    world: self.world,
                    arch,
                    slot,
                };

                list.entry(&arch.entity(slot).unwrap(), &row);
            }
        }

        list.finish()
    }
}
/// Debug formats the specified entities,
/// Created using [World::format_entities]
#[doc(hidden)]
pub struct EntitiesFormatter<'a> {
    pub(crate) world: &'a World,
    pub(crate) ids: &'a [Entity],
}

impl<'a> Debug for EntitiesFormatter<'a> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_map();

        for &id in self.ids {
            let Ok(loc) = self.world.location(id) else {
                continue;
            };

            let arch = self.world.archetypes.get(loc.arch_id);

            let row = RowValueFormatter {
                world: self.world,
                arch,
                slot: loc.slot,
            };

            list.entry(&id, &row);
        }

        list.finish()
    }
}

/// Debug formats an entity using ` { id: { components: values ... } }` style
#[doc(hidden)]
pub struct EntityFormatter<'a> {
    pub(crate) world: &'a World,
    pub(crate) arch: &'a Archetype,
    pub(crate) slot: Slot,
    pub(crate) id: Entity,
}

impl<'a> Debug for EntityFormatter<'a> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_map();

        let row = RowValueFormatter {
            world: self.world,
            slot: self.slot,
            arch: self.arch,
        };

        list.entry(&self.id, &row);

        list.finish()
    }
}

pub(crate) struct MissingDebug;

impl Debug for MissingDebug {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "...")
    }
}

/// Formats all components of a specific entity/slot in the archetype
pub(crate) struct RowValueFormatter<'a> {
    pub world: &'a World,
    pub arch: &'a Archetype,
    pub slot: Slot,
}

struct ComponentName {
    base_name: &'static str,
    id: ComponentKey,
}

impl Debug for ComponentName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.base_name, self.id)
    }
}

impl<'a> Debug for RowValueFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for data in self.arch.try_borrow_all().flatten() {
            let desc = data.storage.desc();

            if let Ok(visitor) = self.world.get(desc.key().id, debuggable()) {
                map.entry(&desc, (visitor.debug_storage)(&data.storage, self.slot));
            } else {
                map.entry(&desc, &MissingDebug);
            }
        }

        map.finish()
    }
}

/// Debug formats the subtree from root using the specified relation
/// Created using [World::format_entities]
#[doc(hidden)]
pub struct HierarchyFormatter<'a> {
    pub(crate) world: &'a World,
    pub(crate) id: Entity,
    pub(crate) slot: Slot,
    pub(crate) arch: &'a Archetype,
    pub(crate) relation: Entity,
}

impl<'a> Debug for HierarchyFormatter<'a> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_map();

        let row = RowValueFormatter {
            world: self.world,
            arch: self.arch,
            slot: self.slot,
        };

        s.entry(&"components", &row);
        s.entry(
            &"children",
            &ChildrenFormatter {
                world: self.world,
                relation: ComponentKey::new(self.relation, Some(self.id)),
            },
        );

        s.finish()
    }
}

struct ChildrenFormatter<'a> {
    world: &'a World,
    relation: ComponentKey,
}

impl<'a> Debug for ChildrenFormatter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_map();

        for (_, arch) in self
            .world
            .archetypes
            .iter()
            .filter(|(_, arch)| arch.has(self.relation))
        {
            for (slot, &id) in arch.entities().iter().enumerate() {
                list.entry(
                    &id,
                    &HierarchyFormatter {
                        world: self.world,
                        id,
                        slot,
                        arch,
                        relation: self.relation.id,
                    },
                );
            }
        }
        list.finish()
    }
}

#[cfg(test)]
mod tests {
    use core::fmt::Write;

    use crate::components::{child_of, name};

    use super::*;

    #[test]
    fn tree_formatter() {
        let mut world = World::new();

        let root = Entity::builder()
            .set(name(), "root".into())
            .attach(
                child_of,
                Entity::builder()
                    .set(name(), "child.1".into())
                    .attach(child_of, Entity::builder().set(name(), "child.1.1".into())),
            )
            .attach(child_of, Entity::builder().set(name(), "child.2".into()))
            .spawn(&mut world);

        let mut s = alloc::string::String::new();
        write!(s, "{:#?}", world.format_hierarchy(child_of, root)).unwrap();

        #[cfg(feature = "std")]
        println!("{}", s)
    }
}
