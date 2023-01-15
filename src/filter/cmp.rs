//! Implements filters for component value comparisons.
//! The difference between these and a normal filter of if inside a for loop is
//! that entities **not** yielded will not be marked as modified.
//!
//! This is not possible using a normal if as the item is changed anyway.
//! An alternative may be a "modify guard", a Notify on Write, or NOW if you
//! want :P.

use core::{
    any::type_name,
    cmp::Ordering,
    fmt::{self, Debug, Display},
};

use alloc::vec;
use alloc::vec::Vec;
use atomic_refcell::AtomicRef;

use crate::{
    archetype::{Slice, Slot},
    fetch::{FetchPrepareData, PreparedFetch},
    Access, Component, ComponentValue, Entity, EntityIds, Fetch, FetchItem,
};

/// A filter which compare a component before yielding an item from the query
pub trait CmpExt<T, Q> {
    /// Filter any component less than `other`.
    fn lt(self, other: T) -> OrdCmp<T, Q>
    where
        T: PartialOrd;
    /// Filter any component greater than `other`.
    fn gt(self, other: T) -> OrdCmp<T, Q>
    where
        T: PartialOrd;
    /// Filter any component greater than or equal to `other`.
    fn ge(self, other: T) -> OrdCmp<T, Q>
    where
        T: PartialOrd;
    /// Filter any component less than or equal to `other`.
    fn le(self, other: T) -> OrdCmp<T, Q>
    where
        T: PartialOrd;
    /// Filter any component equal to `other`.
    fn eq(self, other: T) -> OrdCmp<T, Q>
    where
        T: PartialOrd;
    /// Filter any component by predicate.
    fn cmp<F>(self, func: F) -> Cmp<T, F>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static;
}

impl CmpExt<Entity, EntityIds> for EntityIds {
    fn lt(self, other: Entity) -> OrdCmp<Entity, EntityIds>
    where
        Entity: PartialOrd,
    {
        OrdCmp::new(self, CmpMethod::Less, other)
    }

    fn gt(self, other: Entity) -> OrdCmp<Entity, EntityIds>
    where
        Entity: PartialOrd,
    {
        OrdCmp::new(self, CmpMethod::Greater, other)
    }

    fn ge(self, other: Entity) -> OrdCmp<Entity, EntityIds>
    where
        Entity: PartialOrd,
    {
        OrdCmp::new(self, CmpMethod::GreaterEq, other)
    }

    fn le(self, other: Entity) -> OrdCmp<Entity, EntityIds>
    where
        Entity: PartialOrd,
    {
        OrdCmp::new(self, CmpMethod::LessEq, other)
    }

    fn eq(self, other: Entity) -> OrdCmp<Entity, EntityIds>
    where
        Entity: PartialOrd,
    {
        OrdCmp::new(self, CmpMethod::Eq, other)
    }

    fn cmp<F>(self, func: F) -> Cmp<Entity, F>
    where
        F: Fn(&Entity) -> bool + Send + Sync + 'static,
    {
        todo!()
    }
}

impl<T> CmpExt<T, Component<T>> for Component<T>
where
    T: ComponentValue + Debug,
{
    fn lt(self, other: T) -> OrdCmp<T> {
        OrdCmp::new(self, CmpMethod::Less, other)
    }

    fn gt(self, other: T) -> OrdCmp<T> {
        OrdCmp::new(self, CmpMethod::Greater, other)
    }

    fn ge(self, other: T) -> OrdCmp<T> {
        OrdCmp::new(self, CmpMethod::GreaterEq, other)
    }

    fn le(self, other: T) -> OrdCmp<T> {
        OrdCmp::new(self, CmpMethod::LessEq, other)
    }

    fn eq(self, other: T) -> OrdCmp<T> {
        OrdCmp::new(self, CmpMethod::Eq, other)
    }

    fn cmp<F: Fn(&T) -> bool + Send + Sync + 'static>(self, func: F) -> Cmp<T, F> {
        Cmp {
            component: self,
            func,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CmpMethod {
    Less,
    LessEq,
    Eq,
    GreaterEq,
    Greater,
}

impl Display for CmpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CmpMethod::Less => write!(f, "<"),
            CmpMethod::LessEq => write!(f, "<="),
            CmpMethod::Eq => write!(f, "=="),
            CmpMethod::GreaterEq => write!(f, ">="),
            CmpMethod::Greater => write!(f, ">"),
        }
    }
}

#[derive(Clone)]
pub struct OrdCmp<T, Q = Component<T>> {
    fetch: Q,
    method: CmpMethod,
    other: T,
}

impl<T, Q> Debug for OrdCmp<T, Q>
where
    Q: Debug,
    T: ComponentValue,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OrdCmp")
            .field("fetch", &self.fetch)
            .field("method", &self.method)
            .finish()
    }
}

impl<T, Q> OrdCmp<T, Q> {
    fn new(fetch: Q, method: CmpMethod, other: T) -> Self {
        Self {
            fetch,
            method,
            other,
        }
    }
}

impl<'q, T, Q> FetchItem<'q> for OrdCmp<T, Q>
where
    Q: FetchItem<'q>,
{
    type Item = <Q as FetchItem<'q>>::Item;
}

impl<'w, T, Q> Fetch<'w> for OrdCmp<T, Q>
where
    Q: Fetch<'w>,
    Q: for<'q> FetchItem<'q, Item = &'q T>,
    T: PartialOrd + 'static,
{
    type Prepared = PreparedOrdCmp<'w, T, Q::Prepared>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedOrdCmp {
            fetch: self.fetch.prepare(data)?,
            method: self.method,
            other: &self.other,
        })
    }

    fn filter_arch(&self, arch: &crate::Archetype) -> bool {
        self.fetch.filter_arch(arch)
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        self.fetch.access(data)
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fetch.describe(f)?;
        write!(f, " {}", self.method)
    }

    const MUTABLE: bool = true;

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {}
}

pub struct PreparedOrdCmp<'w, T, Q> {
    fetch: Q,
    method: CmpMethod,
    other: &'w T,
}

impl<'w, 'q, T, Q> PreparedFetch<'q> for PreparedOrdCmp<'w, T, Q>
where
    Q: PreparedFetch<'q, Item = &'q T>,
    T: PartialOrd + 'q,
{
    type Item = Q::Item;

    fn fetch(&mut self, slot: usize) -> Self::Item {
        self.fetch.fetch(slot)
    }

    fn filter_slots(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        let method = &self.method;
        let other = &self.other;
        let mut cmp = |slot: Slot| {
            let val = unsafe { self.fetch.fetch(slot) };

            let ord = val.partial_cmp(other);
            if let Some(ord) = ord {
                match method {
                    CmpMethod::Less => ord == Ordering::Less,
                    CmpMethod::LessEq => ord == Ordering::Less || ord == Ordering::Equal,
                    CmpMethod::Eq => ord == Ordering::Equal,
                    CmpMethod::GreaterEq => ord == Ordering::Greater || ord == Ordering::Equal,
                    CmpMethod::Greater => ord == Ordering::Greater,
                }
            } else {
                false
            }
        };

        // Find the first slot which yield true
        let first = match slots.iter().position(&mut cmp) {
            Some(v) => v,
            None => return Slice::empty(),
        };

        let count = slots
            .iter()
            .skip(first)
            .take_while(|&slot| cmp(slot))
            .count();

        Slice {
            start: slots.start + first,
            end: slots.start + first + count,
        }
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        todo!()
    }
}

#[derive(Clone)]
pub struct Cmp<T, F> {
    component: Component<T>,
    func: F,
}

impl<T, F> Debug for Cmp<T, F>
where
    T: ComponentValue,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cmp")
            .field("component", &self.component)
            .field("func", &type_name::<F>())
            .finish()
    }
}

impl<'q, T: ComponentValue, F> FetchItem<'q> for Cmp<T, F> {
    type Item = &'q T;
}

impl<'w, T, F> Fetch<'w> for Cmp<T, F>
where
    T: ComponentValue,
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    const MUTABLE: bool = false;

    type Prepared = PreparedCmp<'w, T, F>;

    fn prepare(&'w self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        Some(PreparedCmp {
            borrow: data.arch.borrow(self.component.key())?,
            func: &self.func,
        })
    }

    fn filter_arch(&self, archetype: &crate::Archetype) -> bool {
        archetype.has(self.component.key())
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        if self.filter_arch(data.arch) {
            vec![Access {
                kind: crate::AccessKind::Archetype {
                    id: data.arch_id,
                    component: self.component.key(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} <=> {}", self.component.name(), &type_name::<F>())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        todo!()
    }
}

pub struct PreparedCmp<'w, T, F>
where
    T: ComponentValue,
{
    borrow: AtomicRef<'w, [T]>,
    func: &'w F,
}

impl<'q, 'w, T, F> PreparedFetch<'q> for PreparedCmp<'w, T, F>
where
    T: ComponentValue,
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    type Item = &'q T;

    fn fetch(&mut self, slot: usize) -> Self::Item {
        todo!()
    }

    fn filter_slots(&mut self, slots: Slice) -> Slice {
        let cmp = |&slot: &Slot| (self.func)(&self.borrow[slot]);

        // How many entities yielded true
        let mut start = slots.start;
        let count = slots
            .iter()
            .skip_while(|slot| {
                if !cmp(slot) {
                    start += 1;
                    true
                } else {
                    false
                }
            })
            .take_while(cmp)
            .count();

        Slice {
            start,
            end: start + count,
        }
    }

    fn set_visited(&mut self, slots: Slice, change_tick: u32) {
        todo!()
    }
}
