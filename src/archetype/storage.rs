use core::{mem, ptr::NonNull};

use alloc::{
    alloc::alloc, alloc::dealloc, alloc::handle_alloc_error, alloc::realloc, alloc::Layout,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{ComponentInfo, ComponentKey, ComponentValue};

use super::Slot;

/// Type erased but managed component store.
pub(crate) struct Storage {
    data: AtomicRefCell<NonNull<u8>>,
    len: usize,
    cap: usize,
    info: ComponentInfo,
}

impl core::fmt::Debug for Storage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Storage")
            .field("len", &self.len)
            .field("info", &self.info)
            .finish()
    }
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
        if self.len + additional <= old_cap {
            return;
        }

        let new_cap = (self.len + additional).next_power_of_two().max(4);
        assert_ne!(new_cap, 0);

        // tracing::debug!(
        //     "Reserving size: {old_cap}[{}] + {additional} => {new_cap} for: {:?}",
        //     self.len(),
        //     self.info().name()
        // );

        let old_layout =
            Layout::from_size_align(self.info.size() * old_cap, self.info.layout.align()).unwrap();
        let new_layout =
            Layout::from_size_align(self.info.size() * new_cap, self.info.layout.align()).unwrap();

        // Handle zst
        if new_layout.size() == 0 {
            self.cap = new_cap;
            return;
        }

        assert!(new_layout.size() < isize::MAX as usize);

        let ptr = if old_cap == 0 {
            debug_assert_eq!(*self.data.get_mut(), NonNull::dangling());
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

            core::ptr::copy(src, dst, self.info.size())
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

        core::ptr::copy_nonoverlapping(
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
        assert_eq!(self.info.type_id, other.info.type_id);

        // This is faster than copying everything over if there is no elements
        // in self
        if self.len == 0 {
            mem::swap(self, other);
            return;
        }

        self.reserve(other.len);

        core::ptr::copy_nonoverlapping(
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
        assert!(self.info.is::<T>(), "Mismatched types");
        let data = match self.data.try_borrow_mut() {
            Ok(v) => v,
            Err(_) => panic!("Component {} is already borrowed", self.info.name()),
        };

        AtomicRefMut::map(data, |v| {
            core::slice::from_raw_parts_mut(v.as_ptr().cast::<T>(), self.len)
        })
    }

    #[inline(always)]
    pub unsafe fn borrow<T: ComponentValue>(&self) -> AtomicRef<[T]> {
        assert!(self.info.is::<T>(), "Mismatched types");
        let data = match self.data.try_borrow() {
            Ok(v) => v,
            Err(_) => panic!("Component {} is already borrowed mutably", self.info.name()),
        };

        AtomicRef::map(data, |v| {
            core::slice::from_raw_parts(v.as_ptr().cast::<T>(), self.len)
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

    #[inline]
    pub(crate) fn push<T: ComponentValue>(&mut self, item: T) {
        debug_assert!(self.info.is::<T>(), "Mismatched types");
        unsafe {
            self.reserve(1);

            core::ptr::write(self.as_ptr().cast::<T>().add(self.len), item);

            self.len += 1
        }
    }

    /// Changes the id of the stored component.
    /// This is safe as the underlying vtable is not changed, as long as the id
    /// points to a component of the same kind.
    pub(crate) unsafe fn set_id(&mut self, id: ComponentKey) {
        self.info.key = id
    }

    pub(crate) fn capacity(&self) -> usize {
        self.cap
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        self.clear();

        // ZST
        if self.cap == 0 || self.info().size() == 0 {
            return;
        }

        let ptr = self.as_ptr();
        let layout =
            Layout::from_size_align(self.info.size() * self.cap, self.info.layout.align()).unwrap();

        unsafe {
            dealloc(ptr, layout);
        }
    }
}

/// Type erased atomic borrow of a component
pub(crate) struct StorageBorrowDyn<'a> {
    data: AtomicRef<'a, NonNull<u8>>,
    info: ComponentInfo,
    len: usize,
}

impl<'a> StorageBorrowDyn<'a> {
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

    /// Returns the component info for the storage
    pub fn info(&self) -> ComponentInfo {
        self.info
    }

    /// Returns the number of items in the storage
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    /// Returns true if the storage is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
