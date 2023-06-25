use crate::ComponentValue;

use super::Metadata;

component! {
    /// Mutually exclusive relation.
    ///
    /// Ensures only one pair of the relation exists.
    pub exclusive: Exclusive,

    ///// Ensures that for every relation `A => B` the relation `B => A` exists.
    /////
    ///// This creates a bidirectional graph.
    //pub symmetric: Symmetric,

}

/// Mutually exclusive relation.
///
/// Ensures only one pair exists of the relation exists.
pub struct Exclusive;

///// Ensures that for every relation `A => B` the relation `B => A` exists.
/////
///// This creates a bidirectional graph.
//pub struct Symmetric;

impl<T: ComponentValue> Metadata<T> for Exclusive {
    fn attach(_: crate::ComponentInfo, buffer: &mut crate::buffer::ComponentBuffer) {
        buffer.set(exclusive(), Exclusive);
    }
}

// impl<T: ComponentValue> Metadata<T> for Symmetric {
//     fn attach(_: crate::ComponentInfo, buffer: &mut crate::buffer::ComponentBuffer) {
//         buffer.set(exclusive(), Exclusive);
//     }
// }

#[cfg(test)]
mod test {
    use alloc::sync::Arc;

    use super::*;

    component! {
        a(id): Arc<()> => [ Exclusive ],
    }

    #[test]
    #[cfg(feature = "flume")]
    fn exclusive_set() {
        use crate::{
            entity_ids,
            events::{Event, EventKind, EventSubscriber},
            relations_like, Entity, EntityIds, Query, QueryBorrow, RelationExt, Relations, World,
        };
        use alloc::{sync::Arc, vec, vec::Vec};
        use itertools::Itertools;
        use pretty_assertions::assert_eq;

        let mut world = World::new();

        let (tx, rx) = flume::unbounded();
        world.subscribe(
            tx.filter_arch(a.with_relation())
                .filter(|v| v.key.id == a.id()),
        );

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

        let shared = Arc::new(());

        let id1 = world.spawn();
        let id2 = world.spawn();
        let id3 = Entity::builder()
            .set(a(id2), shared.clone())
            .set(a(id2), shared.clone())
            .set(a(id1), shared.clone())
            .spawn(&mut world);

        let mut query = Query::new((entity_ids(), relations_like(a)));

        ensure(
            query.borrow(&world),
            &[(id1, vec![]), (id2, vec![]), (id3, vec![(id1, &shared)])],
        );

        world.set(id1, a(id2), shared.clone()).unwrap();

        assert_eq!(
            rx.drain().collect_vec(),
            [
                Event {
                    id: id3,
                    key: a(id1).key(),
                    kind: EventKind::Added
                },
                Event {
                    id: id1,
                    key: a(id2).key(),
                    kind: EventKind::Added
                }
            ]
        );

        world.set(id3, a(id2), shared.clone()).unwrap();

        ensure(
            query.borrow(&world),
            &[
                (id1, vec![(id2, &shared)]),
                (id2, vec![]),
                (id3, vec![(id2, &shared)]),
            ],
        );

        world.set(id1, a(id3), shared.clone()).unwrap();

        assert_eq!(
            rx.drain().collect_vec(),
            [
                Event {
                    id: id3,
                    key: a(id1).key(),
                    kind: EventKind::Removed
                },
                Event {
                    id: id3,
                    key: a(id2).key(),
                    kind: EventKind::Added
                },
                Event {
                    id: id1,
                    key: a(id2).key(),
                    kind: EventKind::Removed
                },
                Event {
                    id: id1,
                    key: a(id3).key(),
                    kind: EventKind::Added
                },
            ]
        );

        ensure(
            query.borrow(&world),
            &[
                (id1, vec![(id3, &shared)]),
                (id2, vec![]),
                (id3, vec![(id2, &shared)]),
            ],
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
            &[
                (id1, vec![(id1, &shared)]),
                (id2, vec![]),
                (id3, vec![(id2, &shared)]),
            ],
        );

        assert_eq!(
            rx.drain().collect_vec(),
            [
                Event {
                    id: id1,
                    key: a(id3).key(),
                    kind: EventKind::Removed
                },
                Event {
                    id: id1,
                    key: a(id1).key(),
                    kind: EventKind::Added
                }
            ]
        );

        drop(world);

        assert_eq!(
            rx.drain().sorted_by_key(|v| v.id).collect_vec(),
            [
                Event {
                    id: id1,
                    key: a(id1).key(),
                    kind: EventKind::Removed
                },
                Event {
                    id: id3,
                    key: a(id2).key(),
                    kind: EventKind::Removed
                }
            ]
        );

        // Ensure relations where dropped
        assert_eq!(Arc::strong_count(&shared), 1);
    }
}
