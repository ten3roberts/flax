use std::{
    iter::FusedIterator,
    ops::{Deref, DerefMut},
    slice::Iter,
};

use crate::{
    archetype::{ArchetypeId, Slot},
    entity::EntityLocation,
    fetch::{Fetch, PreparedFetch},
    Entity, World,
};

/// Represents a query and state for a given world.
/// The archetypes to visit is cached in the query which means it is more
/// performant to reuse the query than creating a new one.
///
/// The archetype borrowing assures aliasing.
/// Two of the same queries can be run at the same time as long as they don't
/// borrow an archetype's component mutably at the same time.
pub struct Query<Q> {
    // The archetypes to visit
    archetypes: Option<Vec<ArchetypeId>>,
    fetch: Q,
}

impl<Q> Query<Q>
where
    Q: for<'x> Fetch<'x>,
{
    /// Construct a new query which will fetch all items in the given query.

    /// The query can be either a singular component, a tuple of components, or
    /// any other type which implements [crate::Fetch].
    pub fn new(query: Q) -> Self {
        Self {
            archetypes: None,
            fetch: query,
        }
    }

    /// Execute the query on the world.
    pub fn iter<'a>(&'a mut self, world: &'a World) -> QueryIter<'a, Q> {
        let (archetypes, fetch) = self.get_archetypes(world);

        QueryIter {
            archetypes: archetypes.into_iter(),
            current: None,
            fetch,
            world,
        }
    }

    /// Execute the query for a single entity
    pub fn get<'a>(
        &'a self,
        entity: Entity,
        world: &'a World,
    ) -> Option<QueryBorrow<'a, <Q as Fetch<'_>>::Prepared>> {
        let &EntityLocation { archetype, slot } = world.location(entity)?;

        let archetype = world.archetype(archetype);

        let mut fetch = self.fetch.prepare(archetype)?;
        // Aliasing is guaranteed due to fetch being prepared and alive for this
        // instance only
        let item = unsafe { fetch.fetch(slot) };
        Some(QueryBorrow {
            item,
            _fetch: fetch,
        })
    }

    fn get_archetypes(&mut self, world: &World) -> (&[ArchetypeId], &Q) {
        let fetch = &self.fetch;
        (
            self.archetypes.get_or_insert_with(|| {
                world
                    .archetypes()
                    .filter_map(|(id, arch)| if fetch.matches(arch) { Some(id) } else { None })
                    .collect()
            }),
            fetch,
        )
    }
}

pub struct QueryBorrow<'a, F: PreparedFetch<'a>> {
    item: F::Item,
    /// Ensures the borrow is not freed
    _fetch: F,
}

impl<'a, F: PreparedFetch<'a>> Deref for QueryBorrow<'a, F> {
    type Target = F::Item;

    fn deref(&self) -> &Self::Target {
        &self.item
    }
}

impl<'a, F: PreparedFetch<'a>> DerefMut for QueryBorrow<'a, F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.item
    }
}

pub struct ArchIter<'a, Q>
where
    Q: Fetch<'a>,
{
    fetch: Q::Prepared,
    pos: Slot,
    len: Slot,
}

impl<'a, Q> Iterator for ArchIter<'a, Q>
where
    Q: Fetch<'a>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos == self.len {
            return None;
        }

        let item = unsafe { self.fetch.fetch(self.pos) };
        self.pos += 1;
        Some(item)
    }
}

pub struct QueryIter<'a, Q>
where
    Q: Fetch<'a>,
{
    archetypes: Iter<'a, ArchetypeId>,
    world: &'a World,
    current: Option<ArchIter<'a, Q>>,
    fetch: &'a Q,
}

impl<'a, Q> Iterator for QueryIter<'a, Q>
where
    Q: Fetch<'a>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut arch) = self.current {
                if let Some(items) = arch.next() {
                    return Some(items);
                }
            }

            let arch = *self.archetypes.next()?;
            let arch = self.world.archetype(arch);
            let fetch = self.fetch.prepare(arch).unwrap();

            self.current = Some(ArchIter {
                fetch,
                pos: 0,
                len: arch.len(),
            });
        }
    }
}

impl<'a, Q> FusedIterator for QueryIter<'a, Q> where Q: Fetch<'a> {}
