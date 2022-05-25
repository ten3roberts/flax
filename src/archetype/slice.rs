use super::Slot;

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct EntitySlice {
    start: Slot,
    end: Slot,
}

impl EntitySlice {
    /// Creates a new slice of entity slots.
    fn new(start: Slot, end: Slot) -> Self {
        Self { start, end }
    }

    fn empty() -> Self {
        Self { start: 0, end: 0 }
    }

    fn len(&self) -> Slot {
        self.end - self.start
    }

    fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    fn intersect(&self, other: &Self) -> Self {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);

        Self::new(start, end)
    }

    /// Returns the union of two slices if contiguous.
    fn union(&self, other: &Self) -> Option<Self> {
        let start = self.start.min(other.start);
        let end = self.end.max(other.end);

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
    fn difference(&self, other: &Self) -> Option<Self> {
        // Subtract start
        if other.start <= self.start {
            Some(EntitySlice::new((other.end + 1).max(self.start), self.end))
        } else if other.end >= self.end {
            Some(EntitySlice::new(
                self.start,
                (other.start - 1).min(self.end),
            ))
        } else {
            None
        }
        // Self::new((other.end + 1).min(self.start), (other.start).max(self.end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slices() {
        let a = EntitySlice::new(0, 40);
        let b = EntitySlice::new(10, 38);

        let i = a.intersect(&b);
        let i2 = b.intersect(&a);

        assert_eq!(i, EntitySlice::new(10, 38));
        assert_eq!(i2, EntitySlice::new(10, 38));

        let u = a.union(&b);

        assert_eq!(u, Some(EntitySlice::new(0, 40)));

        let a = EntitySlice::new(0, 40);
        let b = EntitySlice::new(10, 79);

        let u = a.union(&b);

        assert_eq!(u, Some(EntitySlice::new(0, 79)));

        let a = EntitySlice::new(40, 382);
        let b = EntitySlice::new(0, 40);

        let u = a.union(&b);

        assert_eq!(u, Some(EntitySlice::new(0, 382)));

        let a = EntitySlice::new(41, 382);
        let b = EntitySlice::new(0, 40);

        let u = a.union(&b);

        assert_eq!(u, None);
    }

    #[test]
    fn slice_intersect() {
        let a = EntitySlice::new(20, 190);
        let b = EntitySlice::new(0, 13);
        let c = EntitySlice::new(0, 30);
        let d = EntitySlice::new(140, 1000);
        let e = EntitySlice::new(30, 121);

        dbg!(a);
        assert_eq!(a.difference(&b), Some(EntitySlice::new(20, 190)));

        assert_eq!(a.difference(&c), Some(EntitySlice::new(31, 190)));

        assert_eq!(a.difference(&a), Some(EntitySlice::new(191, 190)));

        assert_eq!(a.difference(&d), Some(EntitySlice::new(20, 139)));

        assert_eq!(a.difference(&e), None);
    }
}
