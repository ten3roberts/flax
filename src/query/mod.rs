use std::{
    iter::FusedIterator,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    slice::Iter,
};

use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    entity::EntityLocation,
    fetch::{Fetch, PreparedFetch},
    All, Entity, FilterIter, PrepareInfo, World,
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
    archetypes: Vec<ArchetypeId>,
    change_tick: u32,
    archetype_gen: u32,
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
            archetypes: Vec::new(),
            fetch: query,
            change_tick: 0,
            archetype_gen: 0,
        }
    }

    /// Execute the query on the world.
    pub fn iter<'a>(&'a mut self, world: &'a World) -> QueryIter<'a, Q> {
        let change_tick = self.change_tick;
        let (archetypes, fetch) = self.get_archetypes(world);

        QueryIter {
            old_tick: change_tick,
            new_tick: if Q::MUTABLE {
                world.advance_change_tick()
            } else {
                0
            },
            archetypes: archetypes.into_iter(),
            current: None,
            fetch,
            world,
        }
    }

    /// Execute the query for a single entity.
    /// A mutable query will advance the global change tick of the world.
    pub fn get<'a>(
        &'a self,
        entity: Entity,
        world: &'a World,
    ) -> Option<QueryBorrow<'a, <Q as Fetch<'_>>::Prepared>> {
        let &EntityLocation { archetype, slot } = world.location(entity)?;

        let archetype = world.archetype(archetype);

        let info = PrepareInfo {
            old_tick: self.change_tick,
            new_tick: self.change_tick,
            slots: Slice::new(slot, slot),
        };

        let mut fetch = self.fetch.prepare(archetype)?;

        // It is only necessary to acquire a new change tick if the query will
        // change anything
        let new_tick = if Q::MUTABLE {
            world.advance_change_tick()
        } else {
            world.change_tick()
        };

        fetch.set_visited(Slice::new(slot, slot), new_tick);

        // Aliasing is guaranteed due to fetch being prepared and alive for this
        // instance only. The lock is held and causes fetches for the same
        // archetype to fail
        let item = unsafe { fetch.fetch(slot) };

        Some(QueryBorrow {
            item,
            _fetch: fetch,
        })
    }

    fn get_archetypes(&mut self, world: &World) -> (&[ArchetypeId], &Q) {
        let fetch = &self.fetch;
        if world.archetype_gen() > self.archetype_gen {
            self.archetypes.clear();
            self.archetypes
                .extend(world.archetypes().filter_map(|(id, arch)| {
                    if fetch.matches(arch) {
                        Some(id)
                    } else {
                        None
                    }
                }))
        }

        (&self.archetypes, fetch)
    }
}

pub struct ArchetypeIter<'a, F: PreparedFetch<'a>, I: Iterator<Item = Slice> = FilterIter<All>> {
    fetch: F,
    chunks: I,
    current: Option<Chunk<'a, F>>,
    _marker: PhantomData<&'a ()>,
    new_tick: u32,
}

impl<'a, Q, I> Iterator for ArchetypeIter<'a, Q, I>
where
    Q: PreparedFetch<'a> + 'a,
    I: Iterator<Item = Slice>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let current = match self.current {
            Some(ref mut v) => v,
            None => {
                let v = self.chunks.next()?;

                self.fetch.set_visited(v, self.new_tick);

                let chunk = Chunk {
                    pos: v.start,
                    end: v.end,
                    _marker: PhantomData,
                };

                self.current.get_or_insert(chunk)
            }
        };

        current.next(&mut self.fetch)
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

pub struct Chunk<'a, F: PreparedFetch<'a>> {
    pos: Slot,
    end: Slot,
    _marker: PhantomData<&'a F>,
}

impl<'a, F: PreparedFetch<'a>> Chunk<'a, F> {
    fn next(&mut self, fetch: &mut F) -> Option<F::Item> {
        if self.pos == self.end {
            return None;
        }

        let item = unsafe { fetch.fetch(self.pos) };
        self.pos += 1;
        Some(item)
    }
}

pub struct QueryIter<'a, Q>
where
    Q: Fetch<'a>,
{
    old_tick: u32,
    new_tick: u32,
    archetypes: Iter<'a, ArchetypeId>,
    world: &'a World,
    current: Option<ArchetypeIter<'a, Q::Prepared>>,
    fetch: &'a Q,
}

impl<'a, Q> Iterator for QueryIter<'a, Q>
where
    Q: Fetch<'a>,
{
    type Item = Q::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut chunk) = self.current {
                if let Some(items) = chunk.next() {
                    return Some(items);
                }
            }

            let arch = *self.archetypes.next()?;
            let arch = self.world.archetype(arch);

            let fetch = self.fetch.prepare(arch).unwrap();

            self.current = Some(ArchetypeIter {
                fetch,
                chunks: FilterIter::new(arch.slots(), All),
                current: None,
                _marker: PhantomData,
                new_tick: self.new_tick,
            });
        }
    }
}

impl<'a, Q> FusedIterator for QueryIter<'a, Q> where Q: Fetch<'a> {}
