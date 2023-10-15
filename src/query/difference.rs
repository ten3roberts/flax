use core::iter::Peekable;

use crate::{
    archetype::ArchetypeId, component::ComponentDesc, components::component_info, Fetch, World,
};

/// Returns all items in left not in right
struct SetDifference<T, L: Iterator<Item = T>, R: Iterator<Item = T>> {
    left: Peekable<L>,
    right: Peekable<R>,
}

impl<T, L: Iterator<Item = T>, R: Iterator<Item = T>> SetDifference<T, L, R> {
    fn new(left: L, right: R) -> Self {
        Self {
            left: left.peekable(),
            right: right.peekable(),
        }
    }
}

impl<T: Ord, L: Iterator<Item = T>, R: Iterator<Item = T>> Iterator for SetDifference<T, L, R> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (l, r) = match (self.left.peek(), self.right.peek()) {
                (None, None) => return None,
                (None, Some(_)) => return None,
                (Some(_), None) => return self.left.next(),
                (Some(l), Some(r)) => (l, r),
            };

            match l.cmp(r) {
                core::cmp::Ordering::Less => return self.left.next(),
                core::cmp::Ordering::Equal => {
                    self.left.next();
                    self.right.next();
                }
                core::cmp::Ordering::Greater => {
                    self.right.next();
                }
            }
        }
    }
}

pub(crate) fn find_missing_components<'q, 'a, Q>(
    fetch: &Q,
    arch_id: ArchetypeId,
    world: &'a World,
) -> impl Iterator<Item = ComponentDesc> + 'a
where
    Q: Fetch<'a>,
{
    let arch = world.archetypes.get(arch_id);

    let mut searcher = Default::default();
    fetch.searcher(&mut searcher);
    SetDifference::new(
        searcher.required.into_iter(),
        arch.components().map(|v| v.key()),
    )
    .flat_map(|v| world.get(v.id, component_info()).ok().as_deref().cloned())
}

#[cfg(test)]
mod test {
    use itertools::Itertools;

    use super::SetDifference;

    #[test]
    fn difference_iter() {
        let diff = SetDifference::new([1, 2, 4, 5, 6, 8].into_iter(), [1, 2, 6, 7].into_iter())
            .collect_vec();

        assert_eq!(diff, [4, 5, 8]);

        let diff =
            SetDifference::new([1, 2, 6, 7].into_iter(), [1, 2, 3, 6, 7].into_iter()).collect_vec();

        assert_eq!(diff, [0i32; 0]);
    }
}
