use atomic_refcell::AtomicRef;

use crate::{archetype::Slot, component::ComponentValue, system::AccessKind, util::Ptr, Component};

use super::{read_only::RandomFetch, *};

#[doc(hidden)]
pub struct ReadComponent<'a, T> {
    borrow: AtomicRef<'a, [T]>,
}

impl<'q, T: 'q> PreparedFetch<'q> for ReadComponent<'_, T> {
    type Item = &'q T;

    type Chunk = Ptr<'q, T>;

    const HAS_FILTER: bool = false;

    #[inline]
    unsafe fn create_chunk(&'q mut self, slots: Slice) -> Self::Chunk {
        Ptr::new(self.borrow[slots.as_range()].as_ptr())
    }

    #[inline]
    // See: <https://godbolt.org/z/8fWa136b9>
    unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
        let old = chunk.as_ptr();
        chunk.advance(1);
        &*old
    }
}

impl<'q, T: ComponentValue> RandomFetch<'q> for ReadComponent<'_, T> {
    #[inline]
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item {
        self.borrow.get_unchecked(slot)
    }

    #[inline]
    unsafe fn fetch_shared_chunk(chunk: &Self::Chunk, slot: Slot) -> Self::Item {
        chunk.add(slot).as_ref()
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
    fn filter_arch(&self, data: FetchAccessData) -> bool {
        data.arch.has(self.key())
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
