use crate::archetype::Slot;

use super::PreparedFetch;

/// A fetch which only yields items which can freely *alias*.
///
/// This makes the `fetch` method *safer* to implement and can be called with a covariant lifetime.
pub trait ReadOnlyFetch<'q>: PreparedFetch<'q> {
    /// Fetch the shared item from the given slot
    ///
    /// # Safety
    /// Slot must be valid
    unsafe fn fetch_shared(&'q self, slot: Slot) -> Self::Item;
    unsafe fn fetch_shared_chunk(batch: &Self::Chunk, slot: Slot) -> Self::Item;
}
