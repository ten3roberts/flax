use core::slice;
use std::{
    alloc::{alloc, dealloc, handle_alloc_error, realloc, Layout},
    mem,
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{ComponentInfo, ComponentValue};

use super::Slot;

#[derive(Debug)]
/// Type erased but managed component store.
pub(crate) struct Storage {
    data: AtomicRefCell<NonNull<u8>>,
    len: usize,
    cap: usize,
    info: ComponentInfo,
}

impl Storage {
    /// Allocates space for storage of `len` components.
    pub fn new(info: ComponentInfo) -> Self {
        Self::with_capacity(info, 0)
    }

    pub fn with_capacity(info: ComponentInfo, cap: usize) -> Self {
        if cap == 0 {
            return Self {
                data: AtomicRefCell::new(NonNull::dangling()),
                cap: 0,
                len: 0,
                info,
            };
        }

        let layout = Layout::from_size_align(info.size() * cap, info.layout.align()).unwrap();

        unsafe {
            let data = alloc(layout);
            let data = match NonNull::new(data) {
                Some(v) => v,
                None => handle_alloc_error(layout),
            };
            Self {
                data: AtomicRefCell::new(data),
                cap,
                info,
                len: 0,
            }
        }
    }

    /// Allocates more space for the storage
    pub fn reserve(&mut self, additional: usize) {
        let old_cap = self.cap;
        let new_cap = (self.len + additional).next_power_of_two();
        if new_cap <= old_cap {
            return;
        }

        // tracing::debug!(
        //     "Reserving size: {old_cap}[{}] + {additional} => {new_cap} for: {:?}",
        //     self.len(),
        //     self.info().name()
        // );

        let old_layout =
            Layout::from_size_align(self.info.size() * old_cap, self.info.layout.align()).unwrap();
        let new_layout =
            Layout::from_size_align(self.info.size() * new_cap, self.info.layout.align()).unwrap();

        if new_layout.size() == 0 {
            return;
        }

        assert!(new_layout.size() < isize::MAX as usize);

        let ptr = if old_cap == 0 {
            assert_eq!(*self.data.get_mut(), NonNull::dangling());
            unsafe { alloc(new_layout) }
        } else {
            let ptr = self.data.get_mut().as_ptr();
            unsafe { realloc(ptr, old_layout, new_layout.size()) }
        };

        let ptr = match NonNull::new(ptr) {
            Some(v) => v,
            None => handle_alloc_error(new_layout),
        };

        self.cap = new_cap;
        *self.data.get_mut() = ptr
    }

    pub fn swap_remove(&mut self, slot: Slot, on_move: impl FnOnce(*mut u8)) {
        if slot >= self.len() {
            panic!("Index out of bounds")
        }

        unsafe {
            let ptr = self.as_ptr();

            let dst = ptr.add(slot * self.info.size());

            on_move(dst);

            let src = ptr.add((self.len - 1) * self.info.size());

            std::ptr::copy(src, dst, self.info.size())
        }
        self.len -= 1;
    }

    #[inline(always)]
    fn as_ptr(&mut self) -> *mut u8 {
        self.data.get_mut().as_ptr()
    }

    #[inline(always)]
    pub(crate) unsafe fn at_mut(&mut self, slot: Slot) -> Option<*mut u8> {
        if slot >= self.len {
            None
        } else {
            Some(self.data.get_mut().as_ptr().add(self.info.size() * slot))
        }
    }

    #[inline]
    pub(crate) unsafe fn extend(&mut self, src: *mut u8, len: usize) {
        self.reserve(len);

        std::ptr::copy_nonoverlapping(
            src,
            self.as_ptr().add(self.len * self.info.size()),
            len * self.info.size(),
        );

        self.len += len
    }

    /// Appends all items from other to self, leaving other empty.
    ///
    /// # Safety
    /// Other must be of the same type as self
    pub(crate) unsafe fn append(&mut self, other: &mut Self) {
        self.reserve(other.len);

        std::ptr::copy_nonoverlapping(
            other.as_ptr(),
            self.as_ptr().add(self.len * self.info.size()),
            other.len * self.info.size(),
        );

        self.len += other.len;
        other.len = 0;
    }

    #[inline(always)]
    pub(crate) fn info(&self) -> &ComponentInfo {
        &self.info
    }

    #[inline(always)]
    pub unsafe fn borrow_mut<T: ComponentValue>(&self) -> AtomicRefMut<[T]> {
        let data = match self.data.try_borrow_mut() {
            Ok(v) => v,
            Err(_) => panic!("Component {} is already borrowed", self.info.name()),
        };

        AtomicRefMut::map(data, |v| {
            std::slice::from_raw_parts_mut(v.as_ptr().cast::<T>(), self.len)
        })
    }

    #[inline(always)]
    pub unsafe fn borrow<T: ComponentValue>(&self) -> AtomicRef<[T]> {
        let data = match self.data.try_borrow() {
            Ok(v) => v,
            Err(_) => panic!("Component {} is already borrowed mutably", self.info.name()),
        };

        AtomicRef::map(data, |v| {
            std::slice::from_raw_parts(v.as_ptr().cast::<T>(), self.len)
        })
    }

    #[inline(always)]
    pub unsafe fn borrow_dyn(&self) -> StorageBorrowDyn {
        let data = match self.data.try_borrow() {
            Ok(v) => v,
            Err(_) => panic!("Component {} is already borrowed mutably", self.info.name()),
        };

        StorageBorrowDyn {
            data,
            info: self.info,
            len: self.len,
        }
    }

    pub fn clear(&mut self) {
        // Drop all contained valid values
        for slot in 0..self.len {
            unsafe {
                let value = self.at_mut(slot).unwrap();
                (self.info.drop)(value);
            }
        }

        self.len = 0;
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn push<T: ComponentValue>(&mut self, mut item: T) {
        unsafe {
            self.extend(&mut item as *mut T as *mut u8, 1);
        }
        mem::forget(item);
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        self.clear();

        // ZST
        if self.cap == 0 || self.info().size() > 0 {
            return;
        }

        let ptr = self.as_ptr();
        unsafe {
            dealloc(
                ptr,
                Layout::from_size_align(self.info.size() * self.cap, self.info.layout.align())
                    .unwrap(),
            );
        }
    }
}

/// Type erased atomic borrow of a component
pub struct StorageBorrowDyn<'a> {
    data: AtomicRef<'a, NonNull<u8>>,
    info: ComponentInfo,
    len: usize,
}

impl<'a> StorageBorrowDyn<'a> {
    pub fn new(data: AtomicRef<'a, NonNull<u8>>, info: ComponentInfo, len: usize) -> Self {
        Self { data, info, len }
    }

    // pub(crate) unsafe fn downcast<T: ComponentValue>(self) -> AtomicRef<'a, [T]> {
    //     AtomicRef::map(self.data, |v| {
    //         slice::from_raw_parts(v.as_ptr().cast::<T>(), self.len)
    //     })
    // }

    /// Returns a pointer to the value at the given slot.
    ///
    /// Returns None if the slot is out of bounds.
    pub fn at(&self, slot: Slot) -> Option<*const u8> {
        if slot < self.len {
            Some(unsafe { self.data.as_ptr().add(self.info.size() * slot) })
        } else {
            None
        }
    }

    pub fn info(&self) -> ComponentInfo {
        self.info
    }

    /// Convert the type erased storage to a slice of the underlying type.
    ///
    /// # Safety
    /// The type `T` must be the same type as the stored component
    pub unsafe fn as_slice<T>(&self) -> &[T] {
        slice::from_raw_parts(self.data.as_ptr().cast::<T>(), self.len)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
