use alloc::vec::Vec;

use core::fmt::{self, Formatter};

use crate::{
    archetype::{Archetype, CellMutGuard, Slice},
    component::ComponentValue,
    system::{Access, AccessKind},
    util::PtrMut,
    Component, Fetch, FetchItem,
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
    fn filter_arch(&self, data: FetchAccessData) -> bool {
        data.arch.has(self.0.key())
    }

    #[inline]
    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        if data.arch.has(self.0.key()) {
            dst.extend_from_slice(&[Access {
                kind: AccessKind::Archetype {
                    id: data.arch_id,
                    component: self.0.key(),
                },
                mutable: true,
            }])
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

impl<'w, 'q, T: 'q + ComponentValue> PreparedFetch<'q> for WriteComponent<'w, T> {
    type Item = &'q mut T;
    type Chunk = PtrMut<'q, T>;
    
    const HAS_FILTER: bool = false;

    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.guard
            .set_modified(&self.arch.entities[slots.as_range()], slots, self.tick);

        // Convert directly into a non-overlapping subslice without reading the whole slice
        PtrMut::new((self.guard.storage().as_ptr() as *mut T).add(slots.start))
    }

    #[inline]
    // See: <https://godbolt.org/z/8fWa136b9>
    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let old = chunk.as_ptr();
        chunk.advance(1);
        &mut *old
    }
}
