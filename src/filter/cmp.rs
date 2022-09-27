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
    fmt::{self, Debug},
    ops,
};

use alloc::vec;
use alloc::vec::Vec;
use atomic_refcell::AtomicRef;

use crate::{
    archetype::{ArchetypeId, Slice, Slot},
    filter::And,
    filter::Not,
    filter::Or,
    filter::PreparedFilter,
    Access, Archetype, Component, ComponentValue, Filter,
};

/// A filter which compare a component before yielding an item from the query
pub trait CmpExt<T>
where
    T: ComponentValue,
{
    /// Filter any component less than `other`.
    fn lt(self, other: T) -> OrdCmp<T>
    where
        T: PartialOrd;
    /// Filter any component greater than `other`.
    fn gt(self, other: T) -> OrdCmp<T>
    where
        T: PartialOrd;
    /// Filter any component greater than or equal to `other`.
    fn ge(self, other: T) -> OrdCmp<T>
    where
        T: PartialOrd;
    /// Filter any component less than or equal to `other`.
    fn le(self, other: T) -> OrdCmp<T>
    where
        T: PartialOrd;
    /// Filter any component equal to `other`.
    fn eq(self, other: T) -> OrdCmp<T>
    where
        T: PartialOrd;
    /// Filter any component by predicate.
    fn cmp<F>(self, func: F) -> Cmp<T, F>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static;
}

impl<T> CmpExt<T> for Component<T>
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

#[derive(Clone)]
pub struct OrdCmp<T>
where
    T: ComponentValue,
{
    component: Component<T>,
    method: CmpMethod,
    other: T,
}

impl<T> Debug for OrdCmp<T>
where
    T: ComponentValue,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OrdCmp")
            .field("component", &self.component)
            .field("method", &self.method)
            .finish()
    }
}

impl<T> OrdCmp<T>
where
    T: ComponentValue,
{
    fn new(component: Component<T>, method: CmpMethod, other: T) -> Self {
        Self {
            component,
            method,
            other,
        }
    }
}

impl<'w, T> Filter<'w> for OrdCmp<T>
where
    T: ComponentValue + PartialOrd,
{
    type Prepared = PreparedOrdCmp<'w, T>;

    fn prepare(&'w self, archetype: &'w crate::Archetype, _: u32) -> Self::Prepared {
        PreparedOrdCmp {
            borrow: archetype.borrow(self.component),
            method: self.method,
            other: &self.other,
        }
    }

    fn matches(&self, archetype: &crate::Archetype) -> bool {
        archetype.has(self.component.id())
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        if self.matches(archetype) {
            vec![Access {
                kind: crate::AccessKind::Archetype {
                    id,
                    component: self.component.id(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
    }
}

pub struct PreparedOrdCmp<'w, T> {
    borrow: Option<AtomicRef<'w, [T]>>,
    method: CmpMethod,
    other: &'w T,
}

impl<'w, T> PreparedFilter for PreparedOrdCmp<'w, T>
where
    T: ComponentValue + PartialOrd,
{
    fn filter(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        let borrow = match self.borrow {
            Some(ref v) => v,
            None => return Slice::empty(),
        };

        let method = &self.method;
        let other = &self.other;
        let cmp = |&slot: &Slot| {
            let val = &borrow[slot];

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
}

#[derive(Clone)]
pub struct Cmp<T, F>
where
    T: ComponentValue,
{
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

    fn prepare(&'w self, archetype: &'w crate::Archetype, _: u32) -> Self::Prepared {
        PreparedCmp {
            borrow: archetype.borrow(self.component),
            func: &self.func,
        }
    }

    fn matches(&self, archetype: &crate::Archetype) -> bool {
        archetype.has(self.component.id())
    }

    fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
        if self.matches(archetype) {
            vec![Access {
                kind: crate::AccessKind::Archetype {
                    id,
                    component: self.component.id(),
                },
                mutable: false,
            }]
        } else {
            vec![]
        }
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
}

impl<R, T> ops::BitOr<R> for OrdCmp<T>
where
    Self: for<'x> Filter<'x>,
    R: for<'x> Filter<'x>,
    T: ComponentValue + PartialOrd,
{
    type Output = Or<Self, R>;

    fn bitor(self, rhs: R) -> Self::Output {
        Or::new(self, rhs)
    }
}

impl<R, T> ops::BitAnd<R> for OrdCmp<T>
where
    Self: for<'x> Filter<'x>,
    T: ComponentValue + PartialOrd,
    R: for<'x> Filter<'x>,
{
    type Output = And<Self, R>;

    fn bitand(self, rhs: R) -> Self::Output {
        And::new(self, rhs)
    }
}

impl<T> ops::Neg for OrdCmp<T>
where
    Self: for<'x> Filter<'x>,
    T: ComponentValue + PartialOrd,
{
    type Output = Not<Self>;

    fn neg(self) -> Self::Output {
        Not(self)
    }
}

impl<R, T, F> ops::BitOr<R> for Cmp<T, F>
where
    Self: for<'x> Filter<'x>,
    F: Fn(&T) -> bool + Send + Sync + 'static,
    T: ComponentValue + PartialOrd,
    R: for<'x> Filter<'x>,
{
    type Output = Or<Self, R>;

    fn bitor(self, rhs: R) -> Self::Output {
        Or::new(self, rhs)
    }
}

impl<R, T, F> ops::BitAnd<R> for Cmp<T, F>
where
    Self: for<'x> Filter<'x>,
    F: Fn(&T) -> bool + Send + Sync + 'static,
    T: ComponentValue + PartialOrd,
    R: for<'x> Filter<'x>,
{
    type Output = And<Self, R>;

    fn bitand(self, rhs: R) -> Self::Output {
        And::new(self, rhs)
    }
}

impl<T, F> ops::Neg for Cmp<T, F>
where
    Self: for<'x> Filter<'x>,
    F: Fn(&T) -> bool + Send + Sync + 'static,
    T: ComponentValue + PartialOrd,
{
    type Output = Not<Self>;

    fn neg(self) -> Self::Output {
        Not(self)
    }
}
