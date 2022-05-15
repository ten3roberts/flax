use std::alloc::alloc;
use std::{
    alloc::{dealloc, Layout},
    ptr::NonNull,
};

use crate::{archetype::ComponentInfo, util::SparseVec, Component, ComponentValue};

#[derive(Debug)]
pub struct ComponentBuffer {
    /// Stores ComponentId => offset into data
    component_map: SparseVec<(usize, ComponentInfo)>,
    layout: Layout,
    data: NonNull<u8>,
    end: usize, // Number of meaningful bytes
}

impl ComponentBuffer {
    pub fn new() -> Self {
        Self {
            component_map: SparseVec::new(),
            data: NonNull::dangling(),
            end: 0,
            layout: Layout::from_size_align(0, 8).unwrap(),
        }
    }

    pub fn get_mut<T: ComponentValue>(&self, component: Component<T>) -> Option<&mut T> {
        let &(offset, _) = self.component_map.get(component.id().as_raw())?;

        Some(unsafe { &mut *self.data.as_ptr().offset(offset as _).cast() })
    }

    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Option<&T> {
        let &(offset, _) = self.component_map.get(component.id().as_raw())?;

        Some(unsafe { &*self.data.as_ptr().offset(offset as _).cast() })
    }

    pub fn clear(&mut self) {
        for (_, &(offset, info)) in self.component_map.iter() {
            unsafe { (info.drop)(self.data.as_ptr().offset(offset as _)) }
        }

        self.component_map.clear();
        self.end = 0;
    }

    /// # Safety
    /// Take a value from this collection untyped.
    ///
    /// The callee is responsible for dropping. This creates a whole in the
    /// buffer. As such, the buffer should be cleared to free up space.
    pub unsafe fn take_dyn(&mut self, component: &ComponentInfo) -> Option<*mut u8> {
        let (offset, info) = self.component_map.remove(component.id.as_raw())?;
        assert_eq!(&info, component);
        Some(self.data.as_ptr().offset(offset as _))
    }

    pub fn insert<T: ComponentValue>(&mut self, component: Component<T>, value: T) {
        if let Some(&(offset, _)) = self.component_map.get(component.id().as_raw()) {
            unsafe {
                let ptr = self.data.as_ptr().offset(offset as _) as *mut T;
                *ptr = value;
            }
        } else {
            let layout = Layout::new::<T>();
            let offset = self.end + (layout.align() - self.end % layout.align());
            let new_len = offset + layout.size();
            // Reallocate if the current buffer cannot fit an additional
            // T+align bytes
            if new_len >= self.layout.size() {
                // Enforce alignment to be the strictest of all stored types
                let alignment = self.layout.align().max(layout.align());

                let new_layout =
                    Layout::from_size_align(new_len.next_power_of_two(), alignment).unwrap();

                unsafe {
                    // Don't realloc since  layout may change
                    let new_data = alloc(new_layout);

                    if self.layout.size() > 0 {
                        std::ptr::copy_nonoverlapping(self.data.as_ptr(), new_data, self.end);
                        dealloc(self.data.as_ptr(), self.layout)
                    }

                    self.data = NonNull::new(new_data).unwrap();
                }
                self.layout = new_layout;
            }

            // Regardless, the bytes after `len` are allocated and
            // unoccupied
            unsafe {
                let ptr = self.data.as_ptr().offset(offset as _) as *mut T;
                assert_eq!(self.data.as_ptr() as usize % layout.align(), 0);
                assert_eq!(ptr as usize % layout.align(), 0);
                std::ptr::write(ptr, value)
            }
            assert_eq!(
                self.component_map.insert(
                    component.id().as_raw(),
                    (offset, ComponentInfo::of(component))
                ),
                None
            );
            self.end = new_len;
        }
    }
}

impl Default for ComponentBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ComponentBuffer {
    fn drop(&mut self) {
        self.clear();
        if self.layout.size() > 0 {
            unsafe { dealloc(self.data.as_ptr(), self.layout) }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    component! {
        a: i32,
        b: String,
        c: i16,
        d: f32,
        e: [f64; 100],
        f: Arc<String>,
    }

    #[test]
    pub fn component_buffer() {
        let shared = Arc::new("abc".to_string());
        let mut buffer = ComponentBuffer::new();
        buffer.insert(a(), 7);
        buffer.insert(c(), 9);
        buffer.insert(b(), "Hello, World".to_string());
        buffer.insert(e(), [5.0; 100]);

        buffer.insert(f(), shared.clone());

        assert_eq!(buffer.get(a()), Some(&7));
        assert_eq!(buffer.get(c()), Some(&9));
        assert_eq!(buffer.get(b()), Some(&"Hello, World".to_string()));
        assert_eq!(buffer.get(d()), None);
        assert_eq!(buffer.get(e()), Some(&[5.0; 100]));

        drop(buffer);

        assert_eq!(Arc::strong_count(&shared), 1);
    }

    #[test]
    pub fn component_buffer_reinsert() {
        let mut buffer = ComponentBuffer::new();

        let shared = Arc::new("abc".to_string());
        let shared_2 = Arc::new("abc".to_string());
        buffer.insert(f(), shared.clone());
        buffer.insert(f(), shared_2.clone());

        assert_eq!(Arc::strong_count(&shared), 1);
        assert_eq!(Arc::strong_count(&shared_2), 2);
    }
}
