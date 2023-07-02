use crate::archetype::Slot;

use super::PreparedFetch;

/// A fetch which only yields items which can freely *alias*.
///
/// This makes the `fetch` method *safer* to implement and can be called with a covariant lifetime.
pub trait ReadOnlyFetch: PreparedFetch {
    /// Fetch the shared item from the given slot
    ///
    /// # Safety
    /// Slot must be valid
    unsafe fn fetch_shared<'q>(&'q self, slot: Slot) -> Self::Item<'q>;
}

impl<F> ReadOnlyFetch for Option<F>
where
    F: ReadOnlyFetch,
{
    unsafe fn fetch_shared<'q>(&'q self, slot: Slot) -> Self::Item<'q> {
        self.as_ref().map(|fetch| fetch.fetch_shared(slot))
    }
}
