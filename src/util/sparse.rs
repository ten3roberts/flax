use std::{iter::repeat, mem};

pub struct SparseVec<T> {
    sparse: Vec<usize>,
    dense: Vec<(T, usize)>,
}

impl<T> SparseVec<T> {
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
                Some(&mut self.dense[*i - 1].0)
            }
        } else {
            None
        }
    }

    pub fn remove(&mut self, index: u64) -> Option<T> {
        if let Some(i) = self.sparse.get(index as usize) {
            // Swap index with last value
            let i = *i;
            if let Some(back) = self.dense.last() {
                let back_i = back.1;
                // Update the last element to point to the whole of the removed
                // element
                self.sparse[back_i] = i;

                // The last elem is now at i
                let val = self.dense.swap_remove(i - 1).0;
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
                Some(&self.dense[*i - 1].0)
            }
        } else {
            None
        }
    }

    pub fn insert(&mut self, index: u64, value: T) {
        if let Some(val) = self.get_mut(index) {
            mem::replace(val, value);
        } else {
            let i = self.dense.len() + 1;
            self.dense.push((value, i));
            if (self.sparse.len() as u64) < index {
                self.sparse
                    .extend(repeat(0).take(index as usize - self.sparse.len()));

                self.sparse[index as usize] = i
            }
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
        assert_eq!(vec.get(1), Some(&"bar"));

        vec.remove(1);

        assert_eq!(vec.get(2), Some(&"foo"));
        assert_eq!(vec.get(1), None);
        assert_eq!(vec.get(6), Some(&"baz"));
    }
}
