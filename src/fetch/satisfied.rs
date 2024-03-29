use alloc::vec::Vec;

use crate::{archetype::Slice, Fetch, FetchItem};

use super::{FetchAccessData, FmtQuery, PreparedFetch};

/// Yields true iff `F` would match the query
pub struct Satisfied<F>(pub F);

impl<'q, F: FetchItem<'q>> FetchItem<'q> for Satisfied<F> {
    type Item = bool;
}

impl<'w, F: Fetch<'w>> Fetch<'w> for Satisfied<F> {
    const MUTABLE: bool = false;

    type Prepared = PreparedSatisfied<F::Prepared>;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        if self.0.filter_arch(data.into()) {
            Some(PreparedSatisfied(self.0.prepare(data)))
        } else {
            Some(PreparedSatisfied(None))
        }
    }

    fn filter_arch(&self, _: FetchAccessData) -> bool {
        true
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "satisfied {:?}", FmtQuery(&self.0))
    }

    fn access(&self, _: super::FetchAccessData, _: &mut Vec<crate::system::Access>) {}
}

#[doc(hidden)]
pub struct PreparedSatisfied<F>(Option<F>);

impl<'q, F: PreparedFetch<'q>> PreparedFetch<'q> for PreparedSatisfied<F> {
    type Item = bool;
    type Chunk = bool;

    const HAS_FILTER: bool = true;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        match &mut self.0 {
            Some(f) => {
                let res = f.filter_slots(slots);
                !res.is_empty()
            }
            None => false,
        }
    }

    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        *chunk
    }

    unsafe fn filter_slots(&mut self, slots: Slice) -> Slice {
        match &mut self.0 {
            Some(f) => {
                let res = f.filter_slots(slots);

                // Something was missed
                if res.start != slots.start {
                    // Catch the negative slice
                    Slice::new(slots.start, res.start)
                } else {
                    res
                }
            }
            None => slots,
        }
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    use crate::{component, components::name, Entity, FetchExt, Query, World};

    component! {
        a: i32,
    }

    #[test]
    fn satisfied() {
        let mut world = World::new();

        ('a'..='c')
            .map(|v| Entity::builder().set(name(), v.into()).spawn(&mut world))
            .collect_vec();

        ('d'..='f')
            .map(|v| {
                Entity::builder()
                    .set(name(), v.into())
                    .set(a(), 5)
                    .spawn(&mut world)
            })
            .collect_vec();

        let mut query = Query::new((name().cloned(), a().satisfied()));
        assert_eq!(
            query.collect_vec(&world),
            [
                ("a".into(), false),
                ("b".into(), false),
                ("c".into(), false),
                ("d".into(), true),
                ("e".into(), true),
                ("f".into(), true),
            ]
        );
    }

    #[test]
    fn satisfied_modified() {
        let mut world = World::new();

        ('a'..='c')
            .map(|v| Entity::builder().set(name(), v.into()).spawn(&mut world))
            .collect_vec();

        let ids = ('d'..='f')
            .map(|v| {
                Entity::builder()
                    .set(name(), v.into())
                    .set(a(), 5)
                    .spawn(&mut world)
            })
            .collect_vec();

        let mut query = Query::new((name().cloned(), a().modified().satisfied()));

        assert_eq!(
            query.collect_vec(&world),
            [
                ("a".into(), false),
                ("b".into(), false),
                ("c".into(), false),
                ("d".into(), true),
                ("e".into(), true),
                ("f".into(), true),
            ]
        );

        assert_eq!(
            query.collect_vec(&world),
            [
                ("a".into(), false),
                ("b".into(), false),
                ("c".into(), false),
                ("d".into(), false),
                ("e".into(), false),
                ("f".into(), false),
            ]
        );

        *world.get_mut(ids[1], a()).unwrap() = 5;

        assert_eq!(
            query.collect_vec(&world),
            [
                ("a".into(), false),
                ("b".into(), false),
                ("c".into(), false),
                ("d".into(), false),
                ("e".into(), true),
                ("f".into(), false),
            ]
        );
    }
}
