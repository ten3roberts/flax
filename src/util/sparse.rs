use std::{iter::repeat, mem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparseVec<T> {
    sparse: Vec<usize>,
    dense: Vec<(u64, T)>,
}

impl<T> Default for SparseVec<T> {
    fn default() -> Self {
        Self {
            sparse: Default::default(),
            dense: Default::default(),
        }
    }
}

impl<T: std::fmt::Debug> SparseVec<T> {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
        }
    }

    pub fn get_mut(&mut self, index: u64) -> Option<&mut T> {
        if let Some(i) = self.sparse.get(index as usize) {
            if *i == 0 {
                None
            } else {
                Some(&mut self.dense[*i - 1].1)
            }
        } else {
            None
        }
    }

    pub fn remove(&mut self, index: u64) -> Option<T> {
        if let Some(&d_index) = self.sparse.get(index as usize) {
            // Empty slot
            if d_index == 0 {
                return None;
            }
            // Swap index with last value
            if let Some(back) = self.dense.last() {
                let back_i = back.0;
                // Update the last element to point to the hole of the removed
                // element
                println!("back_i: {back_i}, index: {index}");
                self.sparse[back_i as usize] = d_index;
                self.sparse[index as usize] = 0;

                // The last elem is now at i
                let val = self.dense.swap_remove(d_index - 1).1;
                Some(val)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn get(&self, index: u64) -> Option<&T> {
        if let Some(i) = self.sparse.get(index as usize) {
            if *i == 0 {
                None
            } else {
                Some(&self.dense[*i - 1].1)
            }
        } else {
            None
        }
    }

    pub fn insert(&mut self, index: u64, value: T) -> Option<T> {
        if let Some(val) = self.get_mut(index) {
            Some(mem::replace(val, value))
        } else {
            eprintln!("Inserting: {index}, {value:?}");
            let i = self.dense.len() + 1;
            self.dense.push((index, value));
            if (self.sparse.len() as u64) <= index {
                self.sparse
                    .extend(repeat(0).take(index as usize - self.sparse.len() + 1));
            }
            self.sparse[index as usize] = i;
            None
        }
    }

    pub fn clear(&mut self) {
        self.sparse.clear();
        self.dense.clear();
    }

    pub fn iter(&self) -> Iter<T> {
        Iter {
            dense: self.dense.iter(),
        }
    }

    pub fn into_iter(self) -> IntoIter<T> {
        IntoIter {
            dense: self.dense.into_iter(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_vec() {
        let mut vec = SparseVec::new();
        vec.insert(2, "foo");
        vec.insert(1, "bar");
        vec.insert(6, "baz");

        assert_eq!(vec.get(2), Some(&"foo"));
        dbg!(&vec.sparse);
        assert_eq!(vec.get(1), Some(&"bar"));

        let mut iter: Vec<_> = vec.iter().collect();
        iter.sort_by_key(|v| v.0);
        assert_eq!(iter, [(1, &"bar"), (2, &"foo"), (6, &"baz")]);

        assert_eq!(vec.remove(1), Some("bar"));

        assert_eq!(vec.get(2), Some(&"foo"));
        assert_eq!(vec.get(1), None);
        assert_eq!(vec.get(6), Some(&"baz"));
        assert_eq!(vec.insert(2, "Fizz"), Some("foo"));
        let mut iter: Vec<_> = vec.iter().collect();
        iter.sort_by_key(|v| v.0);
        assert_eq!(iter, [(2, &"Fizz"), (6, &"baz")]);
    }
}

pub struct Iter<'a, T> {
    dense: std::slice::Iter<'a, (u64, T)>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (u64, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, val) = self.dense.next()?;
        Some((*index, val))
    }
}

pub struct IntoIter<T> {
    dense: std::vec::IntoIter<(u64, T)>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = (u64, T);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, val) = self.dense.next()?;
        Some((index, val))
    }
}
