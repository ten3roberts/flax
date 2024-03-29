use core::ops::Range;

use alloc::collections::BTreeSet;

use super::Slot;

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Represents a contiguous range of slots within an archetype
pub struct Slice {
    pub(crate) start: Slot,
    pub(crate) end: Slot,
}

impl Slice {
    /// Creates a new slice of entity slots.
    #[inline(always)]
    pub const fn new(start: Slot, end: Slot) -> Self {
        Self { start, end }
    }

    #[inline]
    pub(crate) const fn single(slot: Slot) -> Slice {
        Self::new(slot, slot + 1)
    }

    #[inline]
    /// Returns the number of slots in the slice
    pub fn len(&self) -> Slot {
        self.end.wrapping_sub(self.start)
    }

    #[inline]
    /// Returns true if the slice is empty
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// Convert the slice into a BTreeSet of entities.
    /// If using this in hot loops... don't
    pub fn into_set(self) -> BTreeSet<Slot> {
        BTreeSet::from_iter(self.start..self.end)
    }

    /// Iterate all slots in the slice
    pub fn iter(&self) -> Range<Slot> {
        self.start..self.end
    }

    /// Returns true if the slice contains `slot`
    pub fn contains(&self, slot: Slot) -> bool {
        slot >= self.start && slot < self.end
    }

    #[inline(always)]
    /// Returns the intersection of self and other
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let end = self.end.min(other.end);
        let start = self.start.max(other.start).min(end);

        if start == end {
            None
        } else {
            Some(Self::new(start, end))
        }
    }

    /// Returns the union of two slices if contiguous.
    #[inline(always)]
    pub fn union(&self, other: &Self) -> Option<Self> {
        let start = self.start.min(other.start);
        let end = self.end.max(other.end);

        // 1..2 u 2..3
        if self.end >= other.start && self.start <= other.end {
            Some(Self::new(start, end))
        } else {
            None
        }
    }

    /// Subtract one range from another.
    ///
    /// Returns `None` if `other` is contained within `self` and cannot be
    /// subtracted without splitting.
    #[inline]
    pub fn difference(&self, other: Self) -> Option<Self> {
        //   ====
        // --==
        if other.start <= self.start {
            Some(Self::new(other.end.clamp(self.start, self.end), self.end))
        } else if other.end >= self.end {
            Some(Self::new(
                self.start,
                other.start.clamp(self.start, self.end),
            ))
        } else {
            None
        }

        // if other.start <= self.start {
        //     Some(Slice::new(other.end.max(self.start), self.end))
        // } else if other.end >= self.end {
        //     Some(Slice::new(self.start, other.start.min(self.end)))
        // } else {
        //     None
        // }

        // Self::new((other.end + 1).min(self.start), (other.start).max(self.end))
    }

    /// Split with another slice, returning the left, intersect, and right
    /// portions.
    ///
    /// `other` must be a subset of `self`.
    #[inline]
    pub fn split_with(&self, other: &Self) -> Option<(Self, Self, Self)> {
        if other.start < self.start || other.end > self.end {
            None
        } else {
            Some((
                Self::new(self.start, other.start),
                *other,
                Self::new(other.end, self.end),
            ))
        }
    }

    pub(crate) fn subtract(&self, other: &Self) -> Remainder {
        //    *------*
        // *--*
        if self.end <= other.start || self.start >= other.end {
            Remainder::NoOverlap
        }
        //    *---------*
        // *------------*
        //
        // *---------*
        // *--------------*
        else if other.start <= self.start && other.end >= self.end {
            Remainder::FullOverlap
        }
        // 1.
        //    *-----*
        // *---*
        //
        // *--------*
        // *---*
        else if other.start <= self.start && other.end < self.end {
            Remainder::Right(Self::new(other.end, self.end))
        }
        // 2.
        // *-------*
        //       *---*
        //
        // *-------*
        //     *---*
        else if other.start > self.start && other.end >= self.end {
            Remainder::Left(Self::new(self.start, other.start))
        }
        // 3.
        // *-------*
        //   *---*
        //
        //
        //
        else {
            let left = Self::new(self.start, other.start);
            let right = Self::new(other.end, self.end);
            Remainder::Split(left, right)
        }
    }

    /// Returns true if two slices have a non-zero overlap
    #[inline]
    pub fn overlaps(&self, other: Self) -> bool {
        self.end > other.start && self.start < other.end
    }

    /// Returns true if the slice is a subset of `other`
    pub fn is_subset(&self, other: &Self) -> bool {
        self.is_empty() || (self.start >= other.start && self.end <= other.end)
    }

    /// Converts the slice to a range, useful for slice indexing
    pub fn as_range(&self) -> Range<Slot> {
        self.start..self.end
    }
}

impl core::fmt::Debug for Slice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "({}..{})", self.start, self.end)
    }
}

impl IntoIterator for Slice {
    type Item = Slot;

    type IntoIter = Range<Slot>;

    fn into_iter(self) -> Self::IntoIter {
        self.start..self.end
    }
}

pub(crate) enum Remainder {
    NoOverlap,
    FullOverlap,
    Left(Slice),
    Right(Slice),
    Split(Slice, Slice),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slices() {
        let a = Slice::new(0, 40);
        let b = Slice::new(10, 38);

        let i = a.intersect(&b);
        let i2 = b.intersect(&a);

        assert_eq!(i, Some(Slice::new(10, 38)));
        assert_eq!(i2, Some(Slice::new(10, 38)));
        assert_eq!(Slice::new(10, 20).intersect(&Slice::new(0, 2)), None);

        let u = a.union(&b);

        assert_eq!(u, Some(Slice::new(0, 40)));

        let a = Slice::new(0, 40);
        let b = Slice::new(10, 79);

        let u = a.union(&b);

        assert_eq!(u, Some(Slice::new(0, 79)));

        let a = Slice::new(40, 382);
        let b = Slice::new(0, 40);

        let u = a.union(&b);

        assert_eq!(u, Some(Slice::new(0, 382)));

        let a = Slice::new(40, 382);
        let b = Slice::new(0, 40);

        let u = a.union(&b);

        assert_eq!(u, Some(Slice::new(0, 382)));
    }

    #[test]
    fn slice_intersect() {
        let a = Slice::new(20, 190);
        let b = Slice::new(0, 13);
        let c = Slice::new(0, 30);
        let d = Slice::new(140, 1000);
        let e = Slice::new(30, 121);

        // dbg!(a);
        assert_eq!(a.difference(b), Some(Slice::new(20, 190)));

        assert_eq!(a.difference(c), Some(Slice::new(30, 190)));

        assert_eq!(a.difference(a), Some(Slice::new(190, 190)));

        assert_eq!(a.difference(d), Some(Slice::new(20, 140)));

        assert_eq!(a.difference(e), None);
    }

    #[test]
    fn slice_overlaps() {
        pub fn overlaps(a: Slice, b: Slice) {
            assert!(a.overlaps(b), "a: {a:?} b: {b:?}");
            assert!(b.overlaps(a), "b: {b:?} a: {a:?}");
        }

        pub fn n_overlaps(a: Slice, b: Slice) {
            assert!(!a.overlaps(b), "a: {a:?} b: {b:?}");
            assert!(!b.overlaps(a), "b: {b:?} a: {a:?}");
        }

        n_overlaps(Slice::new(10, 20), Slice::new(0, 10));

        overlaps(Slice::new(0, 50), Slice::new(10, 20));

        overlaps(Slice::new(0, 20), Slice::new(0, 10));

        n_overlaps(Slice::new(68, 85), Slice::new(123, 1000));
    }

    #[test]
    fn union() {
        use Slice as S;
        assert_eq!(S::new(0, 20).union(&S::new(20, 24)), Some(S::new(0, 24)));
        assert_eq!(S::new(0, 20).union(&S::new(19, 24)), Some(S::new(0, 24)));
        assert_eq!(S::new(19, 20).union(&S::new(4, 19)), Some(S::new(4, 20)));
        assert_eq!(S::new(19, 20).union(&S::new(21, 27)), None);
        assert_eq!(S::new(19, 20).union(&S::new(20, 20)), Some(S::new(19, 20)));
        assert_eq!(S::new(19, 20).union(&S::new(0, 0)), None);
    }
}
