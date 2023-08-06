use core::slice;

use atomic_refcell::AtomicRef;

use crate::{archetype::Slot, system::AccessKind, Component, ComponentValue};

use super::{read_only::ReadOnlyFetch, *};

#[doc(hidden)]
pub struct ReadComponent<'a, T> {
    borrow: AtomicRef<'a, [T]>,
}

impl<'w, 'q, T: 'q> PreparedFetch<'q> for ReadComponent<'w, T> {
    type Item = &'q T;

    type Chunk = slice::Iter<'q, T>;

    #[inline]
    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        self.borrow[slots.as_range()].iter()
    }

    #[inline]
    unsafe fn fetch_next(batch: &mut Self::Chunk) -> Self::Item {
        batch.next().unwrap()
    }
}

impl<'w, 'q, T: ComponentValue> ReadOnlyFetch<'q> for ReadComponent<'w, T> {
    #[inline]
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
        self.borrow.get_unchecked(slot)
    }

    #[inline]
    unsafe fn fetch_shared_chunk(batch: &Self::Chunk, slot: Slot) -> Self::Item {
        &batch.as_slice()[slot]
    }
}

impl<'w, T> Fetch<'w> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = ReadComponent<'w, T>;

    #[inline]
    fn prepare(&self, data: FetchPrepareData<'w>) -> Option<Self::Prepared> {
        let borrow = data.arch.borrow(self.key())?;
        Some(ReadComponent {
            borrow: borrow.into_inner(),
        })
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.key())
    }

    fn access(&self, data: FetchAccessData, dst: &mut Vec<Access>) {
        if data.arch.has(self.key()) {
            dst.push(Access {
                kind: AccessKind::Archetype {
                    id: data.arch_id,
                    component: self.key(),
                },
                mutable: false,
            })
        }
    }

    fn describe(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.name())
    }

    fn searcher(&self, searcher: &mut crate::ArchetypeSearcher) {
        searcher.add_required(self.key())
    }
}

impl<'q, T: ComponentValue> FetchItem<'q> for Component<T> {
    type Item = &'q T;
}
