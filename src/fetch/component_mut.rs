use alloc::vec::Vec;

use core::fmt::{self, Formatter};

use crate::{
    archetype::{Archetype, CellMutGuard, Slice, Slot},
    system::{Access, AccessKind},
    Component, ComponentValue, Fetch, FetchItem,
};

use super::{FetchAccessData, FetchPrepareData, PreparedFetch};

#[derive(Debug, Clone)]
/// Mutable component fetch
/// See [crate::Component::as_mut]
pub struct Mutable<T>(pub(crate) Component<T>);

impl<'w, T> Fetch<'w> for Mutable<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = true;

    type Prepared = WriteComponent<'w, T>;

    #[inline]
    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let guard = data.arch.borrow_mut(self.0.key())?;

        Some(WriteComponent {
            guard,
            arch: data.arch,
            tick: data.new_tick,
        })
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.0.key())
    }

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        if data.arch.has(self.0.key()) {
            dst.extend_from_slice(&[
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
            ])
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

#[doc(hidden)]
pub struct WriteComponent<'a, T> {
    guard: CellMutGuard<'a, [T]>,
    arch: &'a Archetype,
    tick: u32,
}

impl<'q, 'w, T: 'q + ComponentValue> PreparedFetch<'q> for WriteComponent<'w, T> {
    type Item = &'q mut T;

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Perform a reborrow
        // Cast from a immutable to a mutable borrow as all calls to this
        // function are guaranteed to be disjoint
        unsafe { &mut *(self.guard.get_unchecked_mut(slot) as *mut T) }
    }

    fn set_visited(&mut self, slots: Slice) {
        self.guard
            .set_modified(&self.arch.entities, slots, self.tick);
    }
}
