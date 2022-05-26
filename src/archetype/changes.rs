use super::EntitySlice;

#[derive(Default, Debug, Clone, PartialEq)]
/// A self compacting change tracking which holds either singular changes or a
/// range of changes, automaticall merging adjacent ones.
pub struct Changes {
    inner: Vec<(EntitySlice, u64)>,
}

impl Changes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, slice: EntitySlice, change_tick: u64) {
        self.inner.retain_mut(|(v, tick)| {
            if *tick < change_tick {
                if let Some(diff) = v.difference(&slice) {
                    eprintln!("Reduced change slice {tick} from {v:?} to {diff:?}");
                    *v = diff;
                }
            }

            !v.is_empty()
        });

        match self.inner.last_mut() {
            Some((v, tick)) if *tick == change_tick => {
                eprintln!("Attempting to unionize");
                if let Some(u) = v.union(&slice) {
                    eprintln!("Union");
                    *v = u
                } else {
                    eprintln!("No union, pushing new");
                    self.inner.push((slice, change_tick))
                }
            }
            _ => {
                eprintln!("Pushing new change for {change_tick}");
                self.inner.push((slice, change_tick))
            }
        }
    }

    /// Returns the changes in the change list at a particular index.
    pub fn get(&mut self, index: usize) -> Option<&(EntitySlice, u64)> {
        self.inner.get(index)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Iterate all changes from oldest to newest
    pub fn iter(&self) -> std::slice::Iter<(EntitySlice, u64)> {
        self.inner.iter()
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;
    #[test]
    fn changes() {
        let mut changes = Changes::new();

        changes.set(EntitySlice::new(0, 5), 1);

        changes.set(EntitySlice::new(70, 92), 2);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [(EntitySlice::new(0, 5), 1), (EntitySlice::new(70, 92), 2)]
        );

        changes.set(EntitySlice::new(3, 5), 3);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                (EntitySlice::new(0, 2), 1),
                (EntitySlice::new(70, 92), 2),
                (EntitySlice::new(3, 5), 3)
            ]
        );

        // Extend previous change
        changes.set(EntitySlice::new(4, 14), 3);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                (EntitySlice::new(0, 2), 1),
                (EntitySlice::new(70, 92), 2),
                (EntitySlice::new(3, 14), 3)
            ]
        );

        // Overwrite almost all
        changes.set(EntitySlice::new(0, 89), 4);

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [(EntitySlice::new(90, 92), 2), (EntitySlice::new(0, 89), 4),]
        );
    }

    #[test]
    fn changes_small() {
        let mut changes = Changes::new();

        for i in 0..239 {
            let perm = (i * (i + 2)) % 300;
            // let perm = i;
            changes.set(EntitySlice::new(perm, perm), i as _)
        }

        changes.set(EntitySlice::new(70, 249), 300);
        changes.set(EntitySlice::new(0, 89), 301);
        changes.set(EntitySlice::new(209, 300), 302);

        eprintln!("Changes: {changes:#?}");
    }

    #[test]
    fn adjacent() {
        let mut changes = Changes::new();

        changes.set(EntitySlice::new(0, 63), 1);
        changes.set(EntitySlice::new(64, 182), 1);
        assert_eq!(
            changes.iter().copied().collect_vec(),
            [(EntitySlice::new(0, 182), 1)]
        );
    }
}
