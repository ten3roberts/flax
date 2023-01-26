use crate::archetype::Slot;

/// Specialization of a prepared fetch which allows peeking
pub trait PeekableFetch<'p> {
    /// An immutable reference or owned type to the peeked item
    type Peek: 'p;

    /// Peek a slot
    /// # Safety
    /// A peek of the same slot should not alias with a reference returned by
    /// [`PreparedFetch::fetch`](crate::fetch::PreparedFetch::fetch).
    unsafe fn peek(&'p self, slot: Slot) -> Self::Peek;
}

impl<'p, F> PeekableFetch<'p> for Option<F>
where
    F: PeekableFetch<'p>,
{
    type Peek = Option<F::Peek>;

    unsafe fn peek(&'p self, slot: Slot) -> Self::Peek {
        self.as_ref().map(|fetch| fetch.peek(slot))
    }
}
