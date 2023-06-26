use crate::{filter::ChangeFilter, Component, ComponentValue, Fetch, FetchItem};

use super::{FmtQuery, PreparedFetch};

/// Transforms any supported fetch or collection of fetch into a fetch which filters modified
/// items.
pub trait ModifiedFetch: for<'w> Fetch<'w> {
    type Modified: for<'x> Fetch<'x> + for<'y> FetchItem<'y, Item = <Self as FetchItem<'y>>::Item>;
    fn transform_modified(self) -> Self::Modified;
}

impl<T: ComponentValue> ModifiedFetch for Component<T> {
    type Modified = ChangeFilter<T>;
    fn transform_modified(self) -> Self::Modified {
        self.modified()
    }
}

impl<A: ModifiedFetch, B: ModifiedFetch> ModifiedFetch for (A, B) {
    type Modified = Union<(A::Modified, B::Modified)>;
    fn transform_modified(self) -> Self::Modified {
        Union((self.0.transform_modified(), self.1.transform_modified()))
    }
}

/// A specific kind of `or` combinator which only *or* combines the returned entities, but not the
/// component filters. This allows the filters to return fetch items side by side like the wrapped
/// fetch would, since all constituent fetches are satisfied, but not necessarily all their entities.
///
/// This is most useful for change queries, where you care about about *any* change, but still
/// require the entity to have all the components, and have them returned despite not all having
/// changed.
pub struct Union<T>(pub T);
impl<'w, A: Fetch<'w>, B: Fetch<'w>> Fetch<'w> for Union<(A, B)> {
    const MUTABLE: bool = A::MUTABLE | B::MUTABLE;

    type Prepared = Union<(A::Prepared, B::Prepared)>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let inner = &self.0;
        Some(Union((inner.0.prepare(data)?, inner.1.prepare(data)?)))
    }

    fn filter_arch(&self, arch: &crate::archetype::Archetype) -> bool {
        let inner = &self.0;
        inner.0.filter_arch(arch) && inner.1.filter_arch(arch)
    }

    fn access(&self, data: super::FetchAccessData, dst: &mut Vec<crate::system::Access>) {
        let inner = &self.0;
        inner.0.access(data, dst);
        inner.1.access(data, dst);
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let inner = &self.0;
        let mut s = f.debug_tuple("Union");
        s.field(&FmtQuery(&inner.0));
        s.field(&FmtQuery(&inner.1));

        s.finish()
    }
}

impl<'w, A: PreparedFetch<'w>, B: PreparedFetch<'w>> PreparedFetch<'w> for Union<(A, B)> {
    type Item = (A::Item, B::Item);

    unsafe fn fetch(&'w mut self, slot: usize) -> Self::Item {
        let inner = &mut self.0;
        (inner.0.fetch(slot), inner.1.fetch(slot))
    }

    unsafe fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        let inner = &mut self.0;

        [(inner.0.filter_slots(slots)), (inner.1.filter_slots(slots))]
            .into_iter()
            .min()
            .unwrap_or_default()
    }

    fn set_visited(&mut self, slots: crate::archetype::Slice) {
        let inner = &mut self.0;
        inner.0.set_visited(slots);
        inner.1.set_visited(slots);
    }
}

impl<'q, A: FetchItem<'q>, B: FetchItem<'q>> FetchItem<'q> for Union<(A, B)> {
    type Item = (A::Item, B::Item);
}

#[cfg(test)]
mod tests {

    use alloc::string::ToString;
    use itertools::Itertools;

    use crate::{component, entity_ids, CommandBuffer, Entity, Query, World};

    use super::*;

    #[test]
    fn query_modified() {
        component! {
            a: i32,
            b: String,
            other: (),
        }

        let mut world = World::new();

        let id1 = Entity::builder()
            .set(a(), 0)
            .set(b(), "Hello".into())
            .spawn(&mut world);

        let id2 = Entity::builder()
            .set(a(), 1)
            .set(b(), "World".into())
            .spawn(&mut world);

        let id3 = Entity::builder()
            // .set(a(), 0)
            .set(b(), "There".into())
            .spawn(&mut world);

        // Force to a different archetype
        let id4 = Entity::builder()
            .set(a(), 2)
            .set(b(), "!".into())
            .tag(other())
            .spawn(&mut world);

        let mut query = Query::new((entity_ids(), (a(), b()).transform_modified()));

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [
                (id1, (&0, &"Hello".to_string())),
                (id2, (&1, &"World".to_string())),
                (id4, (&2, &"!".to_string()))
            ]
        );

        assert_eq!(query.borrow(&world).iter().collect_vec(), []);

        // Get mut *without* a mut deref is not a change
        assert_eq!(*world.get_mut(id2, a()).unwrap(), 1);

        assert_eq!(query.borrow(&world).iter().collect_vec(), []);

        *world.get_mut(id2, a()).unwrap() = 5;

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [(id2, (&5, &"World".to_string()))]
        );

        // Adding the required component to id3 will cause it to be picked up by the query
        let mut cmd = CommandBuffer::new();
        cmd.set(id3, a(), -1).apply(&mut world).unwrap();

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [(id3, (&-1, &"There".to_string()))]
        );

        cmd.set(id3, b(), ":P".into()).apply(&mut world).unwrap();

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [(id3, (&-1, &":P".to_string()))]
        );
    }
}
