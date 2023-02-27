use core::iter::Peekable;

use crate::{component_info, ArchetypeId, ComponentInfo, Fetch, World};

struct DifferenceIter<T, L: Iterator<Item = T>, R: Iterator<Item = T>> {
    left: Peekable<L>,
    right: Peekable<R>,
}

impl<T, L: Iterator<Item = T>, R: Iterator<Item = T>> DifferenceIter<T, L, R> {
    fn new(left: L, right: R) -> Self {
        Self {
            left: left.peekable(),
            right: right.peekable(),
        }
    }
}

impl<T: Ord, L: Iterator<Item = T>, R: Iterator<Item = T>> Iterator for DifferenceIter<T, L, R> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (l, r) = match (self.left.peek(), self.right.peek()) {
                (None, None) => return None,
                (None, Some(_)) => return self.right.next(),
                (Some(_), None) => return self.left.next(),
                (Some(l), Some(r)) => (l, r),
            };

            match l.cmp(r) {
                core::cmp::Ordering::Less => return self.left.next(),
                core::cmp::Ordering::Equal => {
                    self.left.next();
                    self.right.next();
                }
                core::cmp::Ordering::Greater => return self.right.next(),
            }
        }
    }
}

pub(crate) fn find_missing_components<'q, 'a, Q>(
    fetch: &Q,
    arch_id: ArchetypeId,
    world: &'a World,
) -> impl Iterator<Item = ComponentInfo> + 'a
where
    Q: Fetch<'a>,
{
    let arch = world.archetypes.get(arch_id);

    let mut searcher = Default::default();
    fetch.searcher(&mut searcher);
    DifferenceIter::new(
        arch.components().map(|v| v.key()),
        searcher.required.into_iter(),
    )
    .flat_map(|v| world.get(v.id, component_info()).ok().as_deref().cloned())
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use super::DifferenceIter;

    #[test]
    fn difference_iter() {
        let diff = DifferenceIter::new([1, 2, 6, 7].into_iter(), [1, 2, 4, 5, 6, 8].into_iter())
            .collect_vec();
        assert_eq!(diff, [4, 5, 7, 8]);
    }
}
