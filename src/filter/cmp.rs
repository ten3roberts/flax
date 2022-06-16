//! Implements filters for component value comparisons.
//! The difference between these and a normal filter of if inside a for loop is
//! that entities **not** yielded will not be marked as modified.
//!
//! This is not possible using a normal if as the item is changed anyway.
//! An alternative may be a "modify guard", a Notify on Write, or NOW if you
//! want :P.

use crate::{
    archetype::StorageBorrow, Component, ComponentValue, Fetch, Filter, PreparedFetch,
    PreparedFilter,
};

#[derive(Debug, Clone)]
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
}

impl<'this, 'w, T> Filter<'this, 'w> for Cmp<T>
where
    T: ComponentValue,
{
    type Prepared = PreparedCmp<'this, 'w, T>;

    fn prepare(&'this self, archetype: &'w crate::Archetype, change_tick: u32) -> Self::Prepared {
        todo!()
    }
}

pub struct PreparedCmp<'this, 'w, T> {
    borrow: StorageBorrow<'w, T>,
    method: &'this CmpMethod,
}

impl<'this, 'w, T> PreparedFilter for PreparedCmp<'this, 'w, T>
where
    T: ComponentValue,
{
    fn filter(&mut self, slots: crate::archetype::Slice) -> crate::archetype::Slice {
        let end = slots.start;
        let borrow = &self.borrow;

        let method = &self.method;

        // let count = slots.iter().take_while(|slot| {
        //     let val = borrow.at(slot);
        // })
        todo!()
    }
}
