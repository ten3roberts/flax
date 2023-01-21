use core::fmt::{self, Formatter};

use atomic_refcell::AtomicRefMut;

use crate::{
    archetype::{Archetype, Change, Changes, Slice, Slot},
    Access, AccessKind, Component, ComponentValue, Fetch, FetchItem,
};

use super::{peek::PeekableFetch, FetchAccessData, FetchPrepareData, PreparedFetch};

#[doc(hidden)]
pub struct WriteComponent<'a, T> {
    borrow: AtomicRefMut<'a, [T]>,
    changes: AtomicRefMut<'a, Changes>,
    new_tick: u32,
}

#[derive(Debug, Clone)]
/// Mutable component fetch
/// See [crate::Component::as_mut]
pub struct Mutable<T: ComponentValue>(pub(crate) Component<T>);

impl<'w, T> Fetch<'w> for Mutable<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = true;

    type Prepared = WriteComponent<'w, T>;

    #[inline]
    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let (borrow, changes) = data.arch.borrow_mut(self.0)?;

        Some(WriteComponent {
            borrow,
            changes,
            new_tick: data.new_tick,
        })
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.0.key())
    }

    #[inline]
    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        if data.arch.has(self.0.key()) {
            vec![
                Access {
                    kind: AccessKind::Archetype {
                        id: data.arch_id,
                        component: self.0.key(),
                    },
                    mutable: true,
                },
                Access {
                    kind: AccessKind::ChangeEvent {
                        id: data.arch_id,
                        component: self.0.key(),
                    },
                    mutable: true,
                },
            ]
        } else {
            vec![]
        }
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("mut ")?;
        f.write_str(self.0.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        searcher.add_required(self.0.key())
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for Mutable<T> {
    type Item = &'q mut T;
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for WriteComponent<'w, T> {
    type Item = &'q mut T;

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        unsafe { &mut *(self.borrow.get_unchecked_mut(slot) as *mut T) }
    }

    #[inline]
    fn set_visited(&mut self, slots: Slice) {
        self.changes
            .set_modified_if_tracking(Change::new(slots, self.new_tick));
    }
}

impl<'w, 'p, T: ComponentValue> PeekableFetch<'p> for WriteComponent<'w, T> {
    type Peek = &'p T;

    unsafe fn peek(&'p self, slot: Slot) -> Self::Peek {
        self.borrow.get_unchecked(slot)
    }
}
