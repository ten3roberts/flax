use crate::{
    archetype::ChangeKind, filter::ChangeFilter, filter::Union, Component, ComponentValue,
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

    #[test]
    #[cfg(feature = "derive")]
    fn query_modified_struct() {
        use crate::{fetch::Cloned, Component, Fetch};

        component! {
            a: i32,
            b: String,
            other: (),
        }

        #[derive(Fetch)]
        // #[fetch(item_derives = [Debug], transforms = [Modified])]
        struct MyFetch {
            a: Component<i32>,
            b: Cloned<Component<String>>,
        }

        // #[automatically_derived]
        // impl<'w, 'q> crate::fetch::PreparedFetch<'q> for PreparedMyFetch<'w>
        // where
        //     Component<i32>: 'static,
        //     Cloned<Component<String>>: 'static,
        // {
        //     type Item = MyFetchItem<'q>;
        //     type Batch = (
        //         <<Component<i32> as Fetch<'w>>::Prepared as crate::fetch::PreparedFetch<'q>>::Batch,
        //     );
        // }

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

        let query = MyFetch {
            a: a(),
            b: b().cloned(),
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
        use crate::{fetch::Cloned, Component, Fetch};

        component! {
            a: i32,
            b: String,
            other: (),
        }

        #[derive(Fetch)]
        #[fetch(item_derives = [Debug], transforms = [Modified, Added])]
        struct MyFetch {
            a: Component<i32>,
            b: Cloned<Component<String>>,
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

        let query = MyFetch {
            a: a(),
            b: b().cloned(),
        }
        .added()
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

        assert_eq!(query.collect_vec(&world), []);

        world.remove(id2, a()).unwrap();

        assert_eq!(query.collect_vec(&world), []);

        world.set(id2, a(), 5).unwrap();

        assert_eq!(query.collect_vec(&world), [(id2, (5, "World".to_string()))]);

        // Adding the required component to id3 will cause it to be picked up by the query
        let mut cmd = CommandBuffer::new();
        cmd.set(id3, a(), -1).apply(&mut world).unwrap();

        assert_eq!(
            query.collect_vec(&world),
            [(id3, (-1, "There".to_string()))]
        );
    }

    fn test_derive_parse() {
        use crate::{fetch::Cloned, Component, Fetch};

        // #[derive(Fetch)]
        struct MyFetch {
            a: Component<i32>,
            b: Cloned<Component<String>>,
        }
        ///The item returned by MyFetch
        struct MyFetchItem<'q> {
            a: <Component<i32> as crate::fetch::FetchItem<'q>>::Item,
            b: <Cloned<Component<String>> as crate::fetch::FetchItem<'q>>::Item,
        }
        impl<'q> crate::fetch::FetchItem<'q> for MyFetch {
            type Item = MyFetchItem<'q>;
        }
        #[automatically_derived]
        impl<'w> crate::Fetch<'w> for MyFetch
        where
            Component<i32>: 'static,
            Cloned<Component<String>>: 'static,
        {
            const MUTABLE: bool = <Component<i32> as crate::Fetch<'w>>::MUTABLE
                || <Cloned<Component<String>> as crate::Fetch<'w>>::MUTABLE;
            type Prepared = PreparedMyFetch<'w>;
            #[inline]
            fn prepare(
                &'w self,
                data: crate::fetch::FetchPrepareData<'w>,
            ) -> Option<Self::Prepared> {
                Some(Self::Prepared {
                    a: crate::Fetch::prepare(&self.a, data)?,
                    b: crate::Fetch::prepare(&self.b, data)?,
                })
            }
            #[inline]
            fn filter_arch(&self, arch: &crate::archetype::Archetype) -> bool {
                crate::Fetch::filter_arch(&self.a, arch) && crate::Fetch::filter_arch(&self.b, arch)
            }
            fn describe(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut s = f.debug_struct("MyFetch");
                s.field("a", &crate::fetch::FmtQuery(&self.a));
                s.field("b", &crate::fetch::FmtQuery(&self.b));
                s.finish()
            }
            fn access(
                &self,
                data: crate::fetch::FetchAccessData,
                dst: &mut Vec<crate::system::Access>,
            ) {
                crate::Fetch::access(&self.a, data, dst);
                crate::Fetch::access(&self.b, data, dst)
            }
            fn searcher(&self, searcher: &mut crate::query::ArchetypeSearcher) {
                crate::Fetch::searcher(&self.a, searcher);
                crate::Fetch::searcher(&self.b, searcher);
            }
        }
        ///The prepared fetch for MyFetch
        struct PreparedMyFetch<'w> {
            a: <Component<i32> as crate::Fetch<'w>>::Prepared,
            b: <Cloned<Component<String>> as crate::Fetch<'w>>::Prepared,
        }
        #[automatically_derived]
        impl<'w, 'q> crate::fetch::PreparedFetch<'q> for PreparedMyFetch<'w>
        where
            Component<i32>: 'static,
            Cloned<Component<String>>: 'static,
        {
            type Item = MyFetchItem<'q>;
            type Batch = (
                    <<Component<
                        i32,
                    > as crate::fetch::Fetch<
                        'w,
                    >>::Prepared as crate::fetch::PreparedFetch<'q>>::Batch,
                    <<Cloned<
                        Component<String>,
                    > as crate::fetch::Fetch<
                        'w,
                    >>::Prepared as crate::fetch::PreparedFetch<'q>>::Batch,
                );
            #[inline]
            unsafe fn fetch_next(batch: &mut Self::Batch) -> Self::Item {
                Self::Item {
                    a: <<Component<i32> as crate::fetch::Fetch<'w>>::Prepared
                        as crate::fetch::PreparedFetch<'q>
                        > ::fetch_next(&mut batch.0),
                    b: todo!()
                }
            }
            #[inline]
            unsafe fn filter_slots(
                &mut self,
                slots: crate::archetype::Slice,
            ) -> crate::archetype::Slice {
                crate::fetch::PreparedFetch::filter_slots(&mut (&mut self.a, &mut self.b), slots)
            }
            #[inline]
            unsafe fn create_batch(&mut self, slots: crate::archetype::Slice) -> Self::Batch {
                (
                    crate::fetch::PreparedFetch::create_batch(&mut self.a, slots),
                    crate::fetch::PreparedFetch::create_batch(&mut self.b, slots),
                )
            }
        }
    }
}
