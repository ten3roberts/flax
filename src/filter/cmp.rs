//! Implements filters for component value comparisons.
//! The difference between these and a normal filter of if inside a for loop is
//! that entities **not** yielded will not be marked as modified.
//!
//! This is not possible using a normal if as the item is changed anyway.
//! An alternative may be a "modify guard", a Notify on Write, or NOW if you
//! want :P.

use core::{
    any::type_name,
    borrow::Borrow,
    cmp::Ordering,
    fmt::{self, Debug, Display},
    ops::Deref,
};

use alloc::vec;
use alloc::vec::Vec;
use atomic_refcell::AtomicRef;

use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    fetch::{FetchPrepareData, PreparedFetch},
    filter::PreparedFilter,
    Access, Archetype, Component, ComponentValue, Entity, EntityIds, Fetch, Filter,
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

impl<'w, T, Q> Filter<'w> for OrdCmp<T, Q>
where
    Q: Fetch<'w>,
    Q::Prepared: for<'q> PreparedFetch<'q, Item = &'q T>,
    T: PartialOrd + 'w,
{
    type Prepared = PreparedOrdCmp<'w, T, Q::Prepared>;

    fn prepare(&'w self, data: FetchPrepareData<'w>, _: u32) -> Self::Prepared {
        PreparedOrdCmp {
            borrow: self.fetch.prepare(data),
            method: self.method,
            other: &self.other,
        }
    }

    fn matches(&self, arch: &crate::Archetype) -> bool {
        self.fetch.matches(arch)
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        self.fetch.access(data)
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fetch.describe(f)?;
        write!(f, " {}", self.method)
    }
}

pub struct PreparedOrdCmp<'w, T, Q> {
    borrow: Option<Q>,
    method: CmpMethod,
    other: &'w T,
}

impl<'w, T, Q> PreparedFilter for PreparedOrdCmp<'w, T, Q>
where
    Q: for<'x> PreparedFetch<'x, Item = &'x T>,
    T: PartialOrd + 'w,
{
    fn filter(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        let borrow = match self.borrow.as_mut() {
            Some(v) => v,
            None => return Slice::empty(),
        };

        let method = &self.method;
        let other = &self.other;
        let mut cmp = |slot: Slot| {
            let val = unsafe { borrow.fetch(slot) };

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

    fn matches_slot(&mut self, slot: usize) -> bool {
        let borrow = match self.borrow.as_mut() {
            Some(v) => v,
            None => return false,
        };

        let method = &self.method;
        let other = &self.other;

        let val = unsafe { borrow.fetch(slot) };

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

impl<'w, T, F> Filter<'w> for Cmp<T, F>
where
    T: ComponentValue,
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    type Prepared = PreparedCmp<'w, T, F>;

    fn prepare(&'w self, data: FetchPrepareData<'w>, _: u32) -> Self::Prepared {
        PreparedCmp {
            borrow: data.arch.borrow(self.component.key()),
            func: &self.func,
        }
    }

    fn matches(&self, archetype: &crate::Archetype) -> bool {
        archetype.has(self.component.key())
    }

    fn access(&self, data: FetchPrepareData) -> Vec<Access> {
        if self.matches(data.arch) {
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
}

pub struct PreparedCmp<'w, T, F>
where
    T: ComponentValue,
{
    borrow: Option<AtomicRef<'w, [T]>>,
    func: &'w F,
}

impl<'w, T, F> PreparedFilter for PreparedCmp<'w, T, F>
where
    T: ComponentValue,
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    fn filter(&mut self, slots: Slice) -> Slice {
        let borrow = match self.borrow {
            Some(ref v) => v,
            None => return Slice::empty(),
        };

        let cmp = |&slot: &Slot| (self.func)(&borrow[slot]);

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

    fn matches_slot(&mut self, slot: usize) -> bool {
        let borrow = match self.borrow {
            Some(ref v) => v,
            None => return false,
        };

        (self.func)(&borrow[slot])
    }
}
