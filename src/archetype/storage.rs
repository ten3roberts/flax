use std::{
    alloc::{alloc, handle_alloc_error, realloc, Layout},
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{ComponentId, ComponentInfo};

use super::Slot;

#[derive(Debug)]
/// Holds components for a single type
pub(crate) struct Storage {
    data: AtomicRefCell<NonNull<u8>>,
    info: ComponentInfo,
}

impl Storage {
    pub(super) fn new_dangling(info: ComponentInfo) -> Self {
        Self {
            data: AtomicRefCell::new(NonNull::dangling()),
            info,
        }
    }

    /// Allocates space for storage of `len` components.
    pub fn new(len: usize, info: ComponentInfo) -> Self {
        if len == 0 || info.size() == 0 {
            return Self::new_dangling(info);
        }

        let layout = Layout::from_size_align(info.size() * len, info.layout.align()).unwrap();

        unsafe {
            let data = alloc(layout);
            let data = match NonNull::new(data) {
                Some(v) => v,
                None => handle_alloc_error(layout),
            };
            Self {
                data: AtomicRefCell::new(data),
                info,
            }
        }
    }

    /// Allocates more space for the storage
    pub unsafe fn realloc(&mut self, old_len: usize, new_len: usize) {
        assert!(old_len < new_len);

        let old_layout =
            Layout::from_size_align(self.info.size() * old_len, self.info.layout.align()).unwrap();
        let new_layout =
            Layout::from_size_align(self.info.size() * new_len, self.info.layout.align()).unwrap();

        if new_layout.size() == 0 {
            return;
        }

        let ptr = if old_len == 0 {
            assert_eq!(*self.data.get_mut(), NonNull::dangling());
            alloc(new_layout)
        } else {
            let ptr = self.data.get_mut().as_ptr();
            realloc(ptr, old_layout, new_layout.size())
        };
        let ptr = match NonNull::new(ptr) {
            Some(v) => v,
            None => handle_alloc_error(new_layout),
        };
        *self.data.get_mut() = ptr
    }

    #[inline(always)]
    pub fn as_ptr(&mut self) -> *mut u8 {
        self.data.get_mut().as_ptr()
    }

    #[inline(always)]
    pub(crate) unsafe fn at_mut(&mut self, slot: Slot) -> *mut u8 {
        self.data.get_mut().as_ptr().add(self.info.size() * slot)
    }

    #[inline(always)]
    pub(crate) fn info(&self) -> &ComponentInfo {
        &self.info
    }

    #[inline(always)]
    pub fn borrow_mut(&self) -> AtomicRefMut<NonNull<u8>> {
        match self.data.try_borrow_mut() {
            Ok(v) => v,
            Err(_) => panic!("Component {} is already borrowed", self.info.name()),
        }
    }

    #[inline(always)]
    pub fn borrow(&self) -> AtomicRef<NonNull<u8>> {
        match self.data.try_borrow() {
            Ok(v) => v,
            Err(_) => panic!("Component {} is already borrowed mutably", self.info.name()),
        }
    }

    pub fn get_mut(&mut self) -> &mut NonNull<u8> {
        self.data.get_mut()
    }
}

/// Borrow of a single component
pub struct StorageBorrow<'a, T> {
    data: AtomicRef<'a, [T]>,
}

impl<'a, T> StorageBorrow<'a, T> {
    pub fn new(data: AtomicRef<'a, [T]>) -> Self {
        Self { data }
    }

    /// # Panics
    /// If the entity does not exist in the storage
    pub fn at(&self, slot: Slot) -> &T {
        &self.data[slot]
    }
}

pub struct StorageBorrowMut<'a, T> {
    data: AtomicRefMut<'a, [T]>,
}

impl<'a, T> StorageBorrowMut<'a, T> {
    pub fn new(data: AtomicRefMut<'a, [T]>) -> Self {
        Self { data }
    }

    /// # Panics
    /// If the entity does not exist in the storage
    pub fn at_mut(&mut self, slot: Slot) -> &mut T {
        &mut self.data[slot]
    }

    /// # Panics
    /// If the entity does not exist in the storage
    pub fn at(&self, slot: Slot) -> &T {
        &self.data[slot]
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
}
