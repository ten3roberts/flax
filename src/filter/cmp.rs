//! Implements filters for component value comparisons.
//! The difference between these and a normal filter of if inside a for loop is
//! that entities **not** yielded will not be marked as modified.
//!
//! This is not possible using a normal if as the item is changed anyway.
//! An alternative may be a "modify guard", a Notify on Write, or NOW if you
//! want :P.

use std::{cmp::Ordering, fmt::Debug};

use crate::{
    archetype::{Slice, Slot, StorageBorrow},
    And, Component, ComponentValue, Filter, Not, Or, PreparedFilter, World,
};

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
    fn gte(self, other: T) -> OrdCmp<T>
    where
        T: PartialOrd;
    /// Filter any component less than or equal to `other`.
    fn lte(self, other: T) -> OrdCmp<T>
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

    fn gte(self, other: T) -> OrdCmp<T> {
        OrdCmp::new(self, CmpMethod::GreaterEq, other)
    }

    fn lte(self, other: T) -> OrdCmp<T> {
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

#[derive(Debug, Clone)]
pub struct OrdCmp<T>
where
    T: ComponentValue,
{
    component: Component<T>,
    method: CmpMethod,
    other: T,
}

impl<T> OrdCmp<T>
where
    T: ComponentValue + Debug,
{
    fn new(component: Component<T>, method: CmpMethod, other: T) -> Self {
        Self {
            component,
            method,
            other,
        }
    }
}

impl<'this, 'w, T> Filter<'this, 'w> for OrdCmp<T>
where
    T: ComponentValue + PartialOrd,
{
    type Prepared = PreparedOrdCmp<'this, 'w, T>;

    fn prepare(&'this self, archetype: &'w crate::Archetype, _: u32) -> Self::Prepared {
        PreparedOrdCmp {
            borrow: archetype.storage(self.component),
            method: &self.method,
            other: &self.other,
        }
    }

    fn matches(&self, _: &World, archetype: &crate::Archetype) -> bool {
        archetype.has(self.component.id())
    }
}

pub struct PreparedOrdCmp<'this, 'w, T> {
    borrow: Option<StorageBorrow<'w, T>>,
    method: &'this CmpMethod,
    other: &'this T,
}

impl<'this, 'w, T> PreparedFilter for PreparedOrdCmp<'this, 'w, T>
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
            let val = borrow.at(slot);

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

        let res = Slice {
            start,
            end: start + count,
        };

        res
    }
}

#[derive(Debug, Clone)]
pub struct Cmp<T, F>
where
    T: ComponentValue,
{
    component: Component<T>,
    func: F,
}

impl<'this, 'w, T, F> Filter<'this, 'w> for Cmp<T, F>
where
    T: ComponentValue,
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    type Prepared = PreparedCmp<'this, 'w, T, F>;

    fn prepare(&'this self, archetype: &'w crate::Archetype, _: u32) -> Self::Prepared {
        PreparedCmp {
            borrow: archetype.storage(self.component),
            func: &self.func,
        }
    }

    fn matches(&self, _: &World, archetype: &crate::Archetype) -> bool {
        archetype.has(self.component.id())
    }
}

pub struct PreparedCmp<'f, 'w, T, F>
where
    T: ComponentValue,
{
    borrow: Option<StorageBorrow<'w, T>>,
    func: &'f F,
}

impl<'f, 'w, T, F> PreparedFilter for PreparedCmp<'f, 'w, T, F>
where
    T: ComponentValue,
    F: Fn(&T) -> bool + Send + Sync + 'static,
{
    fn filter(&mut self, slots: Slice) -> Slice {
        let borrow = match self.borrow {
            Some(ref v) => v,
            None => return Slice::empty(),
        };

        let cmp = |&slot: &Slot| {
            let val = borrow.at(slot);

            (self.func)(val)
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

        let res = Slice {
            start,
            end: start + count,
        };

        res
    }
}

impl<R, T> std::ops::BitOr<R> for OrdCmp<T>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    T: ComponentValue + PartialOrd,
    R: for<'x, 'y> Filter<'x, 'y>,
{
    type Output = Or<Self, R>;

    fn bitor(self, rhs: R) -> Self::Output {
        self.or(rhs)
    }
}

impl<R, T> std::ops::BitAnd<R> for OrdCmp<T>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    T: ComponentValue + PartialOrd,
    R: for<'x, 'y> Filter<'x, 'y>,
{
    type Output = And<Self, R>;

    fn bitand(self, rhs: R) -> Self::Output {
        self.and(rhs)
    }
}

impl<T> std::ops::Neg for OrdCmp<T>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    T: ComponentValue + PartialOrd,
{
    type Output = Not<Self>;

    fn neg(self) -> Self::Output {
        Not(self)
    }
}

impl<R, T, F> std::ops::BitOr<R> for Cmp<T, F>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    F: Fn(&T) -> bool + Send + Sync + 'static,
    T: ComponentValue + PartialOrd,
    R: for<'x, 'y> Filter<'x, 'y>,
{
    type Output = Or<Self, R>;

    fn bitor(self, rhs: R) -> Self::Output {
        self.or(rhs)
    }
}

impl<R, T, F> std::ops::BitAnd<R> for Cmp<T, F>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    F: Fn(&T) -> bool + Send + Sync + 'static,
    T: ComponentValue + PartialOrd,
    R: for<'x, 'y> Filter<'x, 'y>,
{
    type Output = And<Self, R>;

    fn bitand(self, rhs: R) -> Self::Output {
        self.and(rhs)
    }
}

impl<T, F> std::ops::Neg for Cmp<T, F>
where
    Self: for<'x, 'y> Filter<'x, 'y>,
    F: Fn(&T) -> bool + Send + Sync + 'static,
    T: ComponentValue + PartialOrd,
{
    type Output = Not<Self>;

    fn neg(self) -> Self::Output {
        Not(self)
    }
}
