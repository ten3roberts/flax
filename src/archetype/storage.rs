use core::{mem, ptr::NonNull};

use alloc::{
    alloc::alloc, alloc::dealloc, alloc::handle_alloc_error, alloc::realloc, alloc::Layout,
};

use crate::component::{ComponentDesc, ComponentKey, ComponentValue};

use super::Slot;

/// Type erased but managed component store.
#[doc(hidden)]
pub struct Storage {
    data: NonNull<u8>,
    /// The number of items
    len: usize,
    cap: usize,
    desc: ComponentDesc,
}

impl core::fmt::Debug for Storage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Storage")
            .field("len", &self.len)
            .field("desc", &self.desc)
            .finish()
    }
}

impl Storage {
    /// Allocates space for storage of `len` components.
    pub fn new(desc: ComponentDesc) -> Self {
        Self::with_capacity(desc, 0)
    }

    pub fn with_capacity(desc: ComponentDesc, cap: usize) -> Self {
        if cap == 0 {
            let data = (desc.vtable.dangling)();

            assert_eq!(data.as_ptr() as usize % desc.layout().align(), 0);
            return Self {
                data,
                cap: 0,
                len: 0,
                desc,
            };
        }

        let layout = Layout::from_size_align(desc.size() * cap, desc.align()).unwrap();

        unsafe {
            let data = alloc(layout);
            let data = match NonNull::new(data) {
                Some(v) => v,
                None => handle_alloc_error(layout),
            };
            assert_eq!(data.as_ptr() as usize % desc.layout().align(), 0);
            Self {
                data,
                cap,
                len: 0,
                desc,
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
        //     self.desc().name()
        // );

        let old_layout =
            Layout::from_size_align(self.desc.size() * old_cap, self.desc.align()).unwrap();
        let new_layout =
            Layout::from_size_align(self.desc.size() * new_cap, self.desc.align()).unwrap();

        // Handle zst
        if new_layout.size() == 0 {
            self.cap = new_cap;
            return;
        }

        assert!(new_layout.size() < isize::MAX as usize);

        let ptr = if old_cap == 0 {
            // Old pointer is dangling
            unsafe { alloc(new_layout) }
        } else {
            let ptr = self.data.as_ptr();
            unsafe { realloc(ptr, old_layout, new_layout.size()) }
        };

        let data = match NonNull::new(ptr) {
            Some(v) => v,
            None => handle_alloc_error(new_layout),
        };

        self.cap = new_cap;
        self.data = data
    }

    pub fn swap_remove(&mut self, slot: Slot, on_move: impl FnOnce(*mut u8)) {
        if slot >= self.len() {
            panic!("Index out of bounds")
        }

        unsafe {
            let ptr = self.as_ptr();

            let dst = ptr.add(slot * self.desc.size());

            on_move(dst);

            let src = ptr.add((self.len - 1) * self.desc.size());

            core::ptr::copy(src, dst, self.desc.size())
        }
        self.len -= 1;
    }

    #[inline(always)]
    fn as_ptr(&mut self) -> *mut u8 {
        self.data.as_ptr()
    }

    #[inline(always)]
    pub(crate) unsafe fn at_mut(&mut self, slot: Slot) -> Option<*mut u8> {
        if slot >= self.len {
            None
        } else {
            Some(self.data.as_ptr().add(self.desc.size() * slot))
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn extend(&mut self, src: *mut u8, len: usize) {
        self.reserve(len);

        core::ptr::copy_nonoverlapping(
            src,
            self.as_ptr().add(self.len * self.desc.size()),
            len * self.desc.size(),
        );

        self.len += len
    }

    /// Appends all items from other to self, leaving other empty.
    ///
    /// # Safety
    /// Other must be of the same type as self
    pub(crate) unsafe fn append(&mut self, other: &mut Self) {
        assert_eq!(
            self.desc.type_id(),
            other.desc.type_id(),
            "Mismatched types"
        );

        // This is faster than copying everything over if there is no elements
        // in self
        if self.len == 0 {
            mem::swap(self, other);
            return;
        }

        self.reserve(other.len);

        core::ptr::copy_nonoverlapping(
            other.as_ptr(),
            self.as_ptr().add(self.len * self.desc.size()),
            other.len * self.desc.size(),
        );

        self.len += other.len;
        other.len = 0;
    }

    #[inline(always)]
    pub fn downcast_mut<T: ComponentValue>(&mut self) -> &mut [T] {
        if !self.desc.is::<T>() {
            panic!("Mismatched types");
        }

        unsafe { core::slice::from_raw_parts_mut(self.data.as_ptr().cast::<T>(), self.len) }
    }

    #[inline(always)]
    pub fn downcast_ref<T: ComponentValue>(&self) -> &[T] {
        if !self.desc.is::<T>() {
            panic!("Mismatched types");
        }

        unsafe { core::slice::from_raw_parts(self.data.as_ptr().cast::<T>(), self.len) }
    }

    pub fn clear(&mut self) {
        // Drop all contained valid values
        for slot in 0..self.len {
            unsafe {
                let value = self.at_mut(slot).unwrap();
                self.desc.drop(value);
            }
        }

        self.len = 0;
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    /// Push new data to the storage.
    ///
    /// # Safety
    /// `item` must be of the same type.
    pub(crate) unsafe fn push<T: ComponentValue>(&mut self, item: T) {
        self.reserve(1);

        core::ptr::write(self.as_ptr().cast::<T>().add(self.len), item);

        self.len += 1
    }

    /// Changes the id of the stored component.
    /// This is safe as the underlying vtable is not changed, as long as the id
    /// points to a component of the same kind.
    pub(crate) unsafe fn set_id(&mut self, id: ComponentKey) {
        self.desc.key = id
    }

    pub(crate) fn capacity(&self) -> usize {
        self.cap
    }

    #[inline]
    pub(crate) fn desc(&self) -> ComponentDesc {
        self.desc
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        self.clear();

        // ZST
        if self.cap == 0 || self.desc.size() == 0 {
            return;
        }

        let ptr = self.as_ptr();
        let layout =
            Layout::from_size_align(self.desc.size() * self.cap, self.desc.align()).unwrap();

        unsafe {
            dealloc(ptr, layout);
        }
    }
}

#[cfg(test)]
mod test {
    use core::ptr;

    use alloc::sync::Arc;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::*;
    use alloc::string::String;
    use alloc::string::ToString;

    component! {
        a:i32,
        b:Arc<String>,
    }

    #[test]
    fn push() {
        let mut storage = Storage::new(a().desc());
        unsafe {
            storage.push(5);
            storage.push(7);

            assert_eq!(storage.downcast_ref::<i32>(), [5, 7]);
            storage.swap_remove(0, |v| ptr::drop_in_place(v.cast::<i32>()));

            assert_eq!(storage.downcast_ref::<i32>(), [7]);

            let mut other = Storage::new(a().desc());
            other.push(8);
            other.push(9);
            other.push(10);

            storage.append(&mut other);
            assert_eq!(storage.downcast_ref::<i32>(), [7, 8, 9, 10]);
        }
    }

    #[test]
    fn drop() {
        let v = Arc::new("This is shared".to_string());
        let mut storage = Storage::new(b().desc());
        unsafe {
            storage.push(v.clone());
            storage.push(v.clone());
            storage.push(v.clone());
        }

        assert_eq!(Arc::strong_count(&v), 4);
        mem::drop(storage);
        assert_eq!(Arc::strong_count(&v), 1);
    }
}
