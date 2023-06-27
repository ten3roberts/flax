use crate::{filter::ChangeFilter, filter::Union, Component, ComponentValue, Fetch, FetchItem};

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

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<$($ty: ModifiedFetch,)*> ModifiedFetch for ($($ty,)*) {
            type Modified = Union<($($ty::Modified,)*)>;
            fn transform_modified(self) -> Self::Modified {
                Union(($(self.$idx.transform_modified(),)*))
            }
        }
    };
}

tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }

#[cfg(test)]
mod tests {

    use alloc::string::{String, ToString};
    use itertools::Itertools;

    use crate::{component, entity_ids, CommandBuffer, Entity, FetchExt, Query, World};

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

        let mut query = Query::new((entity_ids(), (a(), b()).modified()));

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
