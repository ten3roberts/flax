use crate::{
    archetype::ChangeKind,
    component::ComponentValue,
    filter::{ChangeFilter, Filtered, NoEntities, Union},
    Component, EntityIds, FetchExt, Mutable,
};

/// Allows transforming a fetch into another.
///
/// For example transforming a tuple or struct fetch into a modified filtering fetch.
/// The generic signifies a marker to use for transforming
pub trait TransformFetch<Method> {
    /// The transformed type.
    ///
    /// May of may not have the same `Item`
    type Output;
    /// Transform the fetch using the provided method
    fn transform_fetch(self, method: Method) -> Self::Output;
}

impl<T: ComponentValue> TransformFetch<Modified> for Component<T> {
    type Output = ChangeFilter<T>;
    fn transform_fetch(self, _: Modified) -> Self::Output {
        self.into_change_filter(ChangeKind::Modified)
    }
}

impl<T: ComponentValue> TransformFetch<Added> for Component<T> {
    type Output = ChangeFilter<T>;
    fn transform_fetch(self, _: Added) -> Self::Output {
        self.into_change_filter(ChangeKind::Added)
    }
}

impl<T: ComponentValue> TransformFetch<Modified> for Mutable<T> {
    type Output = Filtered<Self, NoEntities>;
    fn transform_fetch(self, _: Modified) -> Self::Output {
        self.filtered(NoEntities)
    }
}

impl<T: ComponentValue> TransformFetch<Added> for Mutable<T> {
    type Output = Filtered<Self, NoEntities>;
    fn transform_fetch(self, _: Added) -> Self::Output {
        self.filtered(NoEntities)
    }
}

impl TransformFetch<Modified> for EntityIds {
    type Output = Filtered<Self, NoEntities>;
    fn transform_fetch(self, _: Modified) -> Self::Output {
        self.filtered(NoEntities)
    }
}

impl TransformFetch<Added> for EntityIds {
    type Output = Filtered<Self, NoEntities>;
    fn transform_fetch(self, _: Added) -> Self::Output {
        self.filtered(NoEntities)
    }
}

/// Marker for a fetch which has been transformed to filter modified items.
#[derive(Debug, Clone, Copy)]
pub struct Modified;

/// Marker for a fetch which has been transformed to filter inserted items.
#[derive(Debug, Clone, Copy)]
pub struct Added;

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<$($ty: TransformFetch<Modified>,)*> TransformFetch<Modified> for ($($ty,)*) {
            type Output = Union<($($ty::Output,)*)>;
            fn transform_fetch(self, method: Modified) -> Self::Output {
                Union(($(self.$idx.transform_fetch(method),)*))
            }
        }

        impl<$($ty: TransformFetch<Added>,)*> TransformFetch<Added> for ($($ty,)*) {
            type Output = Union<($($ty::Output,)*)>;
            fn transform_fetch(self, method: Added) -> Self::Output {
                Union(($(self.$idx.transform_fetch(method),)*))
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

        let mut query = Query::new((entity_ids(), (a(), b(), other().as_mut().opt()).modified()));

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [
                (id1, (&0, &"Hello".to_string(), None)),
                (id2, (&1, &"World".to_string(), None)),
                (id4, (&2, &"!".to_string(), Some(&mut ())))
            ]
        );

        assert_eq!(query.borrow(&world).iter().collect_vec(), []);

        // Get mut *without* a mut deref is not a change
        assert_eq!(*world.get_mut(id2, a()).unwrap(), 1);

        assert_eq!(query.borrow(&world).iter().collect_vec(), []);

        *world.get_mut(id2, a()).unwrap() = 5;

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [(id2, (&5, &"World".to_string(), None))]
        );

        // Adding the required component to id3 will cause it to be picked up by the query
        let mut cmd = CommandBuffer::new();
        cmd.set(id3, a(), -1).apply(&mut world).unwrap();

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [(id3, (&-1, &"There".to_string(), None))]
        );

        cmd.set(id3, b(), ":P".into()).apply(&mut world).unwrap();

        assert_eq!(
            query.borrow(&world).iter().collect_vec(),
            [(id3, (&-1, &":P".to_string(), None))]
        );
    }

    #[test]
    #[cfg(feature = "derive")]
    fn query_modified_struct() {
        use crate::{fetch::Cloned, Component, Fetch, Mutable, Opt};

        component! {
            a: i32,
            b: String,
            other: (),
            c: f32,
        }

        #[derive(Fetch)]
        #[fetch(item_derives = [Debug], transforms = [Modified])]
        struct MyFetch {
            a: Component<i32>,
            b: Cloned<Component<String>>,
            c: Mutable<f32>,
            other: Opt<Mutable<()>>,
        }

        let mut world = World::new();

        let id1 = Entity::builder()
            .set(a(), 0)
            .set(b(), "Hello".into())
            .set_default(c())
            .spawn(&mut world);

        let id2 = Entity::builder()
            .set(a(), 1)
            .set(b(), "World".into())
            .set_default(c())
            .spawn(&mut world);

        let id3 = Entity::builder()
            // .set(a(), 0)
            .set(b(), "There".into())
            .set_default(c())
            .spawn(&mut world);

        // Force to a different archetype
        let id4 = Entity::builder()
            .set(a(), 2)
            .set(b(), "!".into())
            .set_default(c())
            .tag(other())
            .spawn(&mut world);

        let query = MyFetch {
            a: a(),
            b: b().cloned(),
            c: c().as_mut(),
            other: other().as_mut().opt(),
        }
        .modified()
        .map(|v| (*v.a, v.b));

        let mut query = Query::new((entity_ids(), query));

        assert_eq!(
            query.collect_vec(&world),
            [
                (id1, (0, "Hello".to_string())),
                (id2, (1, "World".to_string())),
                (id4, (2, "!".to_string()))
            ]
        );

        assert_eq!(query.collect_vec(&world), []);

        // Get mut *without* a mut deref is not a change
        assert_eq!(*world.get_mut(id2, a()).unwrap(), 1);

        assert_eq!(query.collect_vec(&world), []);

        *world.get_mut(id2, a()).unwrap() = 5;

        assert_eq!(query.collect_vec(&world), [(id2, (5, "World".to_string()))]);

        // Adding the required component to id3 will cause it to be picked up by the query
        let mut cmd = CommandBuffer::new();
        cmd.set(id3, a(), -1).apply(&mut world).unwrap();

        assert_eq!(
            query.collect_vec(&world),
            [(id3, (-1, "There".to_string()))]
        );

        cmd.set(id3, b(), ":P".into()).apply(&mut world).unwrap();

        assert_eq!(query.collect_vec(&world), [(id3, (-1, ":P".to_string()))]);
    }

    #[test]
    #[cfg(feature = "derive")]
    fn query_inserted_struct() {
        use crate::{fetch::Cloned, Component, EntityIds, Fetch, Mutable};

        #[derive(Debug)]
        struct Custom;

        component! {
            a: i32,
            b: String,
            c: Custom,
            other: (),
        }

        #[derive(Fetch)]
        #[fetch(item_derives = [Debug], transforms = [Modified, Added])]
        struct MyFetch {
            #[fetch(ignore)]
            id: EntityIds,

            a: Component<i32>,
            b: Cloned<Component<String>>,
            #[fetch(ignore)]
            c: Mutable<Custom>,
        }

        let mut world = World::new();

        let id1 = Entity::builder()
            .set(a(), 0)
            .set(b(), "Hello".into())
            .set(c(), Custom)
            .spawn(&mut world);

        let id2 = Entity::builder()
            .set(a(), 1)
            .set(b(), "World".into())
            .set(c(), Custom)
            .spawn(&mut world);

        let id3 = Entity::builder()
            // .set(a(), 0)
            .set(b(), "There".into())
            .set(c(), Custom)
            .spawn(&mut world);

        // Force to a different archetype
        let id4 = Entity::builder()
            .set(a(), 2)
            .set(b(), "!".into())
            .set(c(), Custom)
            .tag(other())
            .spawn(&mut world);

        let query = MyFetch {
            id: entity_ids(),
            a: a(),
            b: b().cloned(),
            c: c().as_mut(),
        }
        .added()
        .map(|v| (v.id, *v.a, v.b));

        let mut query = Query::new(query);

        assert_eq!(
            query.collect_vec(&world),
            [
                (id1, 0, "Hello".to_string()),
                (id2, 1, "World".to_string()),
                (id4, 2, "!".to_string())
            ]
        );

        assert_eq!(query.collect_vec(&world), []);

        assert_eq!(query.collect_vec(&world), []);

        world.remove(id2, a()).unwrap();

        assert_eq!(query.collect_vec(&world), []);

        world.set(id2, a(), 5).unwrap();

        assert_eq!(query.collect_vec(&world), [(id2, 5, "World".to_string())]);

        // Adding the required component to id3 will cause it to be picked up by the query
        let mut cmd = CommandBuffer::new();
        cmd.set(id3, a(), -1).apply(&mut world).unwrap();

        assert_eq!(query.collect_vec(&world), [(id3, -1, "There".to_string())]);
    }

    #[test]
    #[cfg(feature = "derive")]
    fn test_derive_parse() {
        use crate::{fetch::Cloned, Component, Fetch};

        #[derive(Fetch)]
        struct MyFetch {
            a: Component<i32>,
            b: Cloned<Component<String>>,
        }
    }
}
