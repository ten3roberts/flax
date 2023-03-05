use crate::archetype::Slot;

use super::PreparedFetch;

/// A fetch which only yields items which can freely *alias*.
///
/// This makes the `fetch` method *safer* to implement and can be called with a covariant lifetimes.
pub trait ReadOnlyFetch<'q>: PreparedFetch<'q> {
    /// Fetch the shared item from the given slot
    ///
    /// # Safety
    /// Slot must be valid
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item;
}

impl<'p, F> ReadOnlyFetch<'p> for Option<F>
where
    F: ReadOnlyFetch<'p>,
{
    unsafe fn fetch_shared(&'p self, slot: Slot) -> Self::Item {
        self.as_ref().map(|fetch| fetch.fetch_shared(slot))
    }
}
