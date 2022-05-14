use std::{iter::repeat, mem};

#[derive(Debug, Clone)]
pub struct SparseVec<T> {
    sparse: Vec<usize>,
    dense: Vec<(T, usize)>,
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
                Some(&mut self.dense[*i - 1].0)
            }
        } else {
            None
        }
    }

    pub fn remove(&mut self, index: u64) -> Option<T> {
        if let Some(&d_index) = self.sparse.get(index as usize) {
            // Swap index with last value
            if let Some(back) = self.dense.last() {
                let back_i = back.1;
                // Update the last element to point to the hole of the removed
                // element
                println!("back_i: {back_i}, index: {index}");
                self.sparse[back_i] = d_index;
                self.sparse[index as usize] = 0;

                // The last elem is now at i
                let val = self.dense.swap_remove(d_index - 1).0;
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

    pub fn insert(&mut self, index: u64, value: T) -> Option<T> {
        if let Some(val) = self.get_mut(index) {
            Some(mem::replace(val, value))
        } else {
            eprintln!("Inserting: {index}, {value:?}");
            let i = self.dense.len() + 1;
            self.dense.push((value, index as _));
            if (self.sparse.len() as u64) <= index {
                self.sparse
                    .extend(repeat(0).take(index as usize - self.sparse.len() + 1));
            }
            self.sparse[index as usize] = i;
            None
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

        assert_eq!(vec.remove(1), Some("bar"));

        assert_eq!(vec.get(2), Some(&"foo"));
        assert_eq!(vec.get(1), None);
        assert_eq!(vec.get(6), Some(&"baz"));
        assert_eq!(vec.insert(2, "Fizz"), Some("foo"));
    }
}
