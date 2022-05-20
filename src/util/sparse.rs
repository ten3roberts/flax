use std::{iter::repeat, mem};

pub trait Key: Eq + Ord {
    fn as_usize(&self) -> usize;
}

impl Key for usize {
    fn as_usize(&self) -> usize {
        *self
    }
}

impl Key for u32 {
    fn as_usize(&self) -> usize {
        *self as usize
    }
}

impl Key for u64 {
    fn as_usize(&self) -> usize {
        *self as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparseVec<K, V> {
    sparse: Vec<usize>,
    dense: Vec<(K, V)>,
}

impl<K, V> Default for SparseVec<K, V> {
    fn default() -> Self {
        Self {
            sparse: Default::default(),
            dense: Default::default(),
        }
    }
}

impl<K: Key, V: std::fmt::Debug> SparseVec<K, V> {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
        }
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if let Some(i) = self.sparse.get(key.as_usize()) {
            if *i == 0 {
                None
            } else {
                Some(&mut self.dense[*i - 1].1)
            }
        } else {
            None
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let index = key.as_usize();
        if let Some(&d_index) = self.sparse.get(index) {
            // Empty slot
            if d_index == 0 {
                return None;
            }
            // Swap index with last value
            if let Some(back) = self.dense.last() {
                let back_i = back.0.as_usize();
                // Update the last element to point to the hole of the removed
                // element
                self.sparse[back_i] = d_index;
                self.sparse[index] = 0;

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

    pub fn get(&self, key: &K) -> Option<&V> {
        if let Some(i) = self.sparse.get(key.as_usize()) {
            if *i == 0 {
                None
            } else {
                Some(&self.dense[*i - 1].1)
            }
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let index = key.as_usize();
        if let Some(val) = self.get_mut(&key) {
            Some(mem::replace(val, value))
        } else {
            eprintln!("Inserting: {index}, {value:?}");
            let i = self.dense.len() + 1;
            self.dense.push((key, value));
            if self.sparse.len() <= index {
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

    pub fn iter(&self) -> Iter<K, V> {
        Iter {
            dense: self.dense.iter(),
        }
    }

    pub fn into_iter(self) -> IntoIter<K, V> {
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
        let mut vec = SparseVec::<u32, _>::new();
        vec.insert(2, "foo");
        vec.insert(1, "bar");
        vec.insert(6, "baz");

        assert_eq!(vec.get(&2), Some(&"foo"));
        dbg!(&vec.sparse);
        assert_eq!(vec.get(&1), Some(&"bar"));

        let mut iter: Vec<_> = vec.iter().collect();
        iter.sort_by_key(|v| v.0);
        assert_eq!(iter, [(&1, &"bar"), (&2, &"foo"), (&6, &"baz")]);

        assert_eq!(vec.remove(&1), Some("bar"));

        assert_eq!(vec.get(&2), Some(&"foo"));
        assert_eq!(vec.get(&1), None);
        assert_eq!(vec.get(&6), Some(&"baz"));
        assert_eq!(vec.insert(2, "Fizz"), Some("foo"));
        let mut iter: Vec<_> = vec.iter().collect();
        iter.sort_by_key(|v| v.0);
        assert_eq!(iter, [(&2, &"Fizz"), (&6, &"baz")]);
    }
}

pub struct Iter<'a, K, V> {
    dense: std::slice::Iter<'a, (K, V)>,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, val) = self.dense.next()?;
        Some((index, val))
    }
}

pub struct IntoIter<K, V> {
    dense: std::vec::IntoIter<(K, V)>,
}

impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, val) = self.dense.next()?;
        Some((index, val))
    }
}
