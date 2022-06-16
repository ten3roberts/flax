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
    Component, ComponentValue, Filter, PreparedFilter,
};

pub trait CmpExt<T>
where
    T: ComponentValue,
{
    fn lt(self, other: T) -> Cmp<T>;
    fn gt(self, other: T) -> Cmp<T>;
    fn gte(self, other: T) -> Cmp<T>;
    fn lte(self, other: T) -> Cmp<T>;
    fn eq(self, other: T) -> Cmp<T>;
}

impl<T> CmpExt<T> for Component<T>
where
    T: ComponentValue + Debug,
{
    fn lt(self, other: T) -> Cmp<T> {
        Cmp::new(self, CmpMethod::Less, other)
    }

    fn gt(self, other: T) -> Cmp<T> {
        Cmp::new(self, CmpMethod::Greater, other)
    }

    fn gte(self, other: T) -> Cmp<T> {
        Cmp::new(self, CmpMethod::GreaterEq, other)
    }

    fn lte(self, other: T) -> Cmp<T> {
        Cmp::new(self, CmpMethod::LessEq, other)
    }

    fn eq(self, other: T) -> Cmp<T> {
        Cmp::new(self, CmpMethod::Eq, other)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CmpMethod {
    Less,
    LessEq,
    Eq,
    GreaterEq,
    Greater,
}

#[derive(Debug, Clone)]
pub struct Cmp<T>
where
    T: ComponentValue,
{
    component: Component<T>,
    method: CmpMethod,
    other: T,
}

impl<T> Cmp<T>
where
    T: ComponentValue + Debug,
{
    pub fn new(component: Component<T>, method: CmpMethod, other: T) -> Self {
        Self {
            component,
            method,
            other,
        }
    }
}

impl<'this, 'w, T> Filter<'this, 'w> for Cmp<T>
where
    T: ComponentValue + PartialOrd + Debug,
{
    type Prepared = PreparedCmp<'this, 'w, T>;

    fn prepare(&'this self, archetype: &'w crate::Archetype, change_tick: u32) -> Self::Prepared {
        PreparedCmp {
            borrow: archetype.storage(self.component),
            method: &self.method,
            other: &self.other,
        }
    }
}

pub struct PreparedCmp<'this, 'w, T> {
    borrow: Option<StorageBorrow<'w, T>>,
    method: &'this CmpMethod,
    other: &'this T,
}

impl<'this, 'w, T> PreparedFilter for PreparedCmp<'this, 'w, T>
where
    T: ComponentValue + PartialOrd + Debug,
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
            eprintln!("Comparing {val:?}");

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
        let count = slots
            .iter()
            .skip_while(|slot| !cmp(slot))
            .take_while(cmp)
            .count();

        let res = Slice {
            start: slots.start,
            end: slots.start + count,
        };

        eprintln!("Original: {slots:?} ==> {res:?}");
        res
    }
}
