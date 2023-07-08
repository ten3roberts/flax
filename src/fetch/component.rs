use atomic_refcell::AtomicRef;

use crate::{archetype::Slot, system::AccessKind, Component, ComponentValue};

use super::{read_only::ReadOnlyFetch, *};

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

impl<'w, 'p, T: ComponentValue> ReadOnlyFetch<'p> for ReadComponent<'w, T> {
    unsafe fn fetch_shared(&'p self, slot: Slot) -> Self::Item {
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
