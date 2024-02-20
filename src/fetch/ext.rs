use crate::{
    component::ComponentValue,
    filter::{Cmp, Equal, Filtered, Greater, GreaterEq, Less, LessEq},
    relation::RelationExt,
    Fetch, FetchItem,
};

use super::{
    as_deref::AsDeref,
    cloned::Cloned,
    copied::Copied,
    opt::{Opt, OptOr},
    source::{FetchSource, FromRelation, Traverse},
    transform::Added,
    Map, Modified, Satisfied, Source, TransformFetch,
};

/// Extension trait for [crate::Fetch]
pub trait FetchExt: Sized {
    /// Transform the fetch into an optional fetch, yielding Some or None
    fn opt(self) -> Opt<Self> {
        Opt { fetch: self }
    }

    /// Transform the fetch into a fetch with a provided default.
    /// This is useful for default values such as scale or velocity which may
    /// not exist for every entity.
    fn opt_or<V>(self, default: V) -> OptOr<Self, V>
    where
        Self: for<'w> Fetch<'w>,
        for<'q> Self: FetchItem<'q, Item = &'q V>,
    {
        OptOr::new(self, default)
    }

    /// Returns true if the query is satisfied, without borrowing
    fn satisfied(self) -> Satisfied<Self> {
        Satisfied(self)
    }

    /// Transform the fetch into a fetch which yields the default impl if the
    /// fetch is not matched.
    fn opt_or_default<V>(self) -> OptOr<Self, V>
    where
        Self: for<'w> Fetch<'w>,
        for<'q> Self: FetchItem<'q, Item = &'q V>,
        V: Default,
    {
        self.opt_or(Default::default())
    }

    /// Transform this into a cloned fetch
    fn cloned(self) -> Cloned<Self>
    where
        Cloned<Self>: for<'x> Fetch<'x>,
    {
        Cloned(self)
    }

    /// Transform this into a copied fetch
    fn copied(self) -> Copied<Self>
    where
        Copied<Self>: for<'x> Fetch<'x>,
    {
        Copied(self)
    }

    /// Dereferences the fetch item
    fn deref(self) -> AsDeref<Self>
    where
        AsDeref<Self>: for<'x> Fetch<'x>,
    {
        AsDeref(self)
    }

    /// Filter any component by predicate.
    fn cmp<F>(self, func: F) -> Cmp<Self, F>
    where
        for<'x> Cmp<Self, F>: Fetch<'x>,
    {
        Cmp::new(self, func)
    }

    /// Filter any component less than `other`.
    fn lt<T>(self, other: T) -> Cmp<Self, Less<T>>
    where
        for<'x> Cmp<Self, Less<T>>: Fetch<'x>,
    {
        Cmp::new(self, Less(other))
    }
    /// Filter any component greater than `other`.
    fn gt<T>(self, other: T) -> Cmp<Self, Greater<T>>
    where
        for<'x> Cmp<Self, GreaterEq<T>>: Fetch<'x>,
    {
        Cmp::new(self, Greater(other))
    }
    /// Filter any component greater than or equal to `other`.
    fn ge<T>(self, other: T) -> Cmp<Self, GreaterEq<T>>
    where
        for<'x> Cmp<Self, GreaterEq<T>>: Fetch<'x>,
    {
        Cmp::new(self, GreaterEq(other))
    }
    /// Filter any component less than or equal to `other`.
    fn le<T>(self, other: T) -> Cmp<Self, LessEq<T>>
    where
        for<'x> Cmp<Self, LessEq<T>>: Fetch<'x>,
    {
        Cmp::new(self, LessEq(other))
    }
    /// Filter any component equal to `other`.
    fn eq<T>(self, other: T) -> Cmp<Self, Equal<T>>
    where
        for<'x> Cmp<Self, Equal<T>>: Fetch<'x>,
    {
        Cmp::new(self, Equal(other))
    }

    /// Set the source entity for the fetch.
    ///
    /// This allows fetching or joining queries
    fn source<S>(self, source: S) -> Source<Self, S>
    where
        S: FetchSource,
    {
        Source::new(self, source)
    }

    /// Follows a relation to resolve the fetch.
    ///
    /// This allows you to for example fetch from the parent of an entity.
    fn relation<T, R>(self, relation: R) -> Source<Self, FromRelation>
    where
        R: RelationExt<T>,
        T: ComponentValue,
    {
        Source::new(
            self,
            FromRelation {
                relation: relation.id(),
                name: relation.vtable().name,
            },
        )
    }

    /// Traverse the edges of a relation recursively to find the first entity which matches the fetch
    ///
    /// This will attempt to resolve a fetch from and including the source entity, to the roots of the relation.
    fn traverse<T, R>(self, relation: R) -> Source<Self, Traverse>
    where
        R: RelationExt<T>,
        T: ComponentValue,
    {
        Source::new(
            self,
            Traverse {
                relation: relation.id(),
            },
        )
    }
    /// Transform the fetch into a fetch where each constituent part tracks and yields for
    /// modification events.
    ///
    /// This is different from E.g; `(a().modified(), b().modified())` as it implies only when
    /// *both* `a` and `b` are modified in the same iteration, which is seldom useful.
    ///
    /// This means will yield *any* of `a` *or* `b` are modified.
    ///
    /// Works with `opt`, `copy`, etc constituents.
    fn modified(self) -> <Self as TransformFetch<Modified>>::Output
    where
        Self: TransformFetch<Modified>,
    {
        self.transform_fetch(Modified)
    }

    /// Transform the fetch into a fetch where each constituent part tracks and yields for
    /// component addition events.
    ///
    /// This is different from E.g; `(a().modified(), b().modified())` as it implies only when
    /// *both* `a` and `b` are modified in the same iteration, which is seldom useful.
    ///
    /// This means will yield *any* of `a` *or* `b` are modified.
    ///
    /// Works with `opt`, `copy`, etc constituents.
    fn added(self) -> <Self as TransformFetch<Added>>::Output
    where
        Self: TransformFetch<Added>,
    {
        self.transform_fetch(Added)
    }
    /// Map each item of the query to another type using the provided function.
    fn map<F, T>(self, func: F) -> Map<Self, F>
    where
        Self: for<'x> FetchItem<'x>,
        for<'x> F: Fn(<Self as FetchItem<'x>>::Item) -> T,
    {
        Map { query: self, func }
    }

    /// Filter a fetch with another fetch as predicate
    fn filtered<F>(self, filter: F) -> Filtered<Self, F>
    where
        F: for<'x> Fetch<'x>,
    {
        Filtered::new(self, filter, true)
    }
}

impl<F> FetchExt for F where F: for<'x> Fetch<'x> {}
