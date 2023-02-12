use atomic_refcell::AtomicRef;

use crate::{archetype::Slot, AccessKind, Component, ComponentValue};

use super::{peek::PeekableFetch, *};

#[doc(hidden)]
pub struct ReadComponent<'a, T> {
    borrow: AtomicRef<'a, [T]>,
}

impl<'q, 'w, T: 'q> PreparedFetch<'q> for ReadComponent<'w, T> {
    type Item = &'q T;

    #[inline(always)]
    unsafe fn fetch(&'q mut self, slot: Slot) -> Self::Item {
        // Safety: bounds guaranteed by callee
        unsafe { self.borrow.get_unchecked(slot) }
    }
}

impl<'w, 'p, T: ComponentValue> PeekableFetch<'p> for ReadComponent<'w, T> {
    type Peek = &'p T;

    unsafe fn peek(&'p self, slot: Slot) -> Self::Peek {
        self.borrow.get_unchecked(slot)
    }
}

impl<'w, T> Fetch<'w> for Component<T>
where
    T: ComponentValue,
{
    const MUTABLE: bool = false;

    type Prepared = ReadComponent<'w, T>;

    #[inline]
    fn prepare(&self, data: FetchPrepareData<'w>) -> Self::Prepared {
        let borrow = data.arch.borrow(self.key()).unwrap();
        ReadComponent { borrow }
    }

    #[inline]
    fn filter_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.key())
    }

    #[inline]
    fn access(&self, data: FetchAccessData) -> Vec<Access> {
        if data.arch.has(self.key()) {
            vec![Access {
                kind: AccessKind::Archetype {
                    id: data.arch_id,
                    component: self.key(),
                },
                mutable: false,
            }]
        } else {
            vec![]
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
