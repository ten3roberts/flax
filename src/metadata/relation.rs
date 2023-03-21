use crate::{ComponentValue, Metadata};

component! {
    /// Mutually exclusive relation.
    ///
    /// Ensures only one pair of the relation exists.
    pub exclusive: Exclusive,

    /// Ensures that for every relation `A => B` the relation `B => A` exists.
    ///
    /// This creates a bidirectional graph.
    pub symmetric: Symmetric,

}

/// Mutually exclusive relation.
///
/// Ensures only one pair exists of the relation exists.
pub struct Exclusive;

/// Ensures that for every relation `A => B` the relation `B => A` exists.
///
/// This creates a bidirectional graph.
pub struct Symmetric;

impl<T: ComponentValue> Metadata<T> for Exclusive {
    fn attach(_: crate::ComponentInfo, buffer: &mut crate::buffer::ComponentBuffer) {
        buffer.set(exclusive(), Exclusive);
    }
}

impl<T: ComponentValue> Metadata<T> for Symmetric {
    fn attach(_: crate::ComponentInfo, buffer: &mut crate::buffer::ComponentBuffer) {
        buffer.set(exclusive(), Exclusive);
    }
}

#[cfg(test)]
mod test {
    use alloc::sync::Arc;
    use itertools::Itertools;

    use crate::{
        entity_ids, relations_like, Entity, EntityIds, Query, QueryBorrow, Relations, World,
    };

    use super::*;

    component! {
        a(id): Arc<()> => [ Exclusive ],
    }

    #[test]
    fn exclusive_set() {
        let mut world = World::new();

        let id1 = world.spawn();
        let id2 = world.spawn();
        let id3 = world.spawn();
        let shared = Arc::new(());

        world.set(id1, a(id2), shared.clone()).unwrap();

        type Expected<'a> = &'a [(Entity, Vec<(Entity, &'a Arc<()>)>)];
        fn ensure(mut query: QueryBorrow<(EntityIds, Relations<Arc<()>>)>, expected: Expected) {
            assert_eq!(
                query
                    .iter()
                    .map(|v| (v.0, v.1.collect_vec()))
                    .sorted()
                    .collect_vec(),
                expected
            );
        }

        let mut query = Query::new((entity_ids(), relations_like(a)));

        ensure(
            query.borrow(&world),
            &[(id1, vec![(id2, &shared)]), (id2, vec![]), (id3, vec![])],
        );

        world.set(id1, a(id3), shared.clone()).unwrap();

        ensure(
            query.borrow(&world),
            &[(id1, vec![(id3, &shared)]), (id2, vec![]), (id3, vec![])],
        );

        Entity::builder()
            .set(a(id2), shared.clone())
            .set(a(id1), shared.clone())
            .set(a(id3), shared.clone())
            .set(a(id1), shared.clone())
            .set(a(id1), shared.clone())
            .append_to(&mut world, id1)
            .unwrap();

        ensure(
            query.borrow(&world),
            &[(id1, vec![(id1, &shared)]), (id2, vec![]), (id3, vec![])],
        );

        drop(world);

        // Ensure relations where dropped
        assert_eq!(Arc::strong_count(&shared), 1);
    }
}
