use std::{
    alloc::{alloc, dealloc, Layout},
    any::type_name,
    ptr::NonNull,
};

use crate::{entity::EntityLocation, util::SparseVec, Component, ComponentId, ComponentValue};

pub type ArchetypeId = u32;
pub type Slot = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct Archetype {
    component_map: Box<[usize]>,
    components: Box<[ComponentInfo]>,
    storage: Box<[Storage]>,
    // Number of entities in the archetype
    len: usize,
    // Number of slots
    cap: usize,
}

impl Archetype {
    pub fn new(mut components: Vec<ComponentInfo>) -> Self {
        components.sort_by_key(|v| v.id);

        let max_component = components.last().unwrap();

        let mut component_map = vec![0; max_component.id.as_raw() as usize + 1].into_boxed_slice();

        let storage = components
            .iter()
            .enumerate()
            .map(|(i, component)| {
                component_map[component.id.as_raw() as usize] = i + 1;
                Storage {
                    data: NonNull::dangling(),
                }
            })
            .collect();

        Self {
            len: 0,
            cap: 0,
            component_map,
            components: components.into_boxed_slice(),
            storage,
        }
    }

    pub fn storage_mut<T: ComponentValue>(
        &mut self,
        component: Component<T>,
    ) -> StorageBorrowMut<T> {
        let index = *self
            .component_map
            .get(component.id().as_raw() as usize)
            .unwrap();

        if index == 0 {
            panic!("Component does not exist");
        }

        let storage = &mut self.storage[(index - 1)];

        // Type is guaranteed by `component_map`
        let data =
            unsafe { std::slice::from_raw_parts_mut(storage.data.as_ptr() as *mut T, self.len) };

        StorageBorrowMut {
            data,
            id: component.id(),
        }
    }

    pub fn storage<T: ComponentValue>(&self, component: Component<T>) -> StorageBorrow<T> {
        let index = *self
            .component_map
            .get(component.id().as_raw() as usize)
            .unwrap();

        if index == 0 {
            panic!("Component does not exist");
        }

        let storage = &self.storage[(index - 1)];

        // Type is guaranteed by `component_map`
        let data =
            unsafe { std::slice::from_raw_parts(storage.data.as_ptr() as *const T, self.len) };

        StorageBorrow {
            data,
            id: component.id(),
        }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get<T: ComponentValue + std::fmt::Debug>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> &T {
        let storage = self.storage(component);

        let value = &storage.data[slot];
        value
    }

    /// Insert a new entity into the archetype.
    /// The components must be a superset of the archetype. Other components
    /// will be ignored.
    ///
    /// Returns the index of the entity
    pub fn insert(&mut self, mut components: ComponentBuffer) -> Slot {
        let slot = self.len;
        self.len += 1;

        // Make sure a new component will fit
        self.reserve(1);

        // Insert all components
        unsafe {
            for (storage, component) in self.storage.iter_mut().zip(self.components.iter()) {
                let src = components
                    .take_dyn(&component)
                    .expect(&format!("Missing component: {component:?}"));

                let dst = storage.elem_raw(slot, &component);

                std::ptr::copy_nonoverlapping(src, dst, component.layout.size());
            }
        }

        slot
    }

    /// Reserves space for atleast `additional` entities.
    /// Does nothing if the remaining capacity < additional.
    /// len remains unchanged, as does the internal order
    pub fn reserve(&mut self, additional: usize) {
        let old_cap = self.cap;
        let new_cap = (self.len + additional).next_power_of_two();

        if new_cap <= old_cap {
            return;
        }

        unsafe {
            for (storage, component) in self.storage.iter_mut().zip(self.components.iter()) {
                let new_layout = Layout::from_size_align(
                    component.layout.size() * new_cap,
                    component.layout.align(),
                )
                .unwrap();
                let new_data = alloc(new_layout);

                if old_cap > 0 {
                    // Copy over the previous contiguous data
                    std::ptr::copy_nonoverlapping(
                        storage.data.as_ptr(),
                        new_data,
                        component.layout.size() * self.len,
                    );

                    dealloc(
                        storage.data.as_ptr(),
                        Layout::from_size_align(
                            component.layout.size() * old_cap,
                            component.layout.align(),
                        )
                        .unwrap(),
                    );
                }

                storage.data = NonNull::new(new_data).unwrap();
            }
        }

        self.cap = new_cap;
    }

    pub fn clear(&mut self) {
        for (storage, component) in self.storage.iter_mut().zip(self.components.iter()) {
            for slot in 0..self.len {
                unsafe {
                    let value = storage.data.as_ptr().add(slot * component.layout.size());
                    (component.drop)(value);
                }
            }
        }
    }

    /// Get the archetype's len.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Get the archetype's cap.
    #[must_use]
    pub fn cap(&self) -> usize {
        self.cap
    }
}

impl Drop for Archetype {
    fn drop(&mut self) {
        self.clear();
        if self.cap > 0 {
            for (storage, component) in self.storage.iter_mut().zip(self.components.iter()) {
                // Handle ZST
                if component.layout.size() > 0 {
                    unsafe {
                        dealloc(
                            storage.data.as_ptr(),
                            Layout::from_size_align(
                                component.layout.size() * self.cap,
                                component.layout.align(),
                            )
                            .unwrap(),
                        );
                    }
                }
            }
        }
    }
}

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
        for (id, &(offset, info)) in self.component_map.iter() {
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

/// Borrow of a single component
pub struct StorageBorrow<'a, T> {
    data: &'a [T],
    id: ComponentId,
}

pub struct StorageBorrowMut<'a, T> {
    data: &'a mut [T],
    id: ComponentId,
}

#[derive(Debug, Clone, PartialEq)]
/// Holds components for a single type
struct Storage {
    data: NonNull<u8>,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct ComponentInfo {
    type_name: &'static str,
    layout: Layout,
    id: ComponentId,
    drop: unsafe fn(*mut u8),
}

impl ComponentInfo {
    pub fn of<T: ComponentValue>(component: Component<T>) -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }
        Self {
            drop: drop_ptr::<T>,
            type_name: type_name::<T>(),
            layout: Layout::new::<T>(),
            id: component.id(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::component;

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

    #[test]
    pub fn test_archetype() {
        let mut arch = Archetype::new(vec![ComponentInfo::of(b()), ComponentInfo::of(a())]);

        let mut buffer = ComponentBuffer::new();
        buffer.insert(a(), 7);
        buffer.insert(b(), "Foo".to_string());
        let slot = arch.insert(buffer);

        assert_eq!(arch.get(slot, a()), &7);
        assert_eq!(arch.get(slot, b()), "Foo");
    }
}

impl Storage {
    /// # Safety
    /// Assumes the type `T` is compatible with the stored type.
    /// `len` is the length of the allocated slice in T
    unsafe fn as_slice_mut<T>(&mut self, len: usize) -> &mut [T] {
        std::slice::from_raw_parts_mut(self.data.as_ptr().cast(), len)
    }

    /// # Safety
    /// Assumes the type `T` is compatible with the stored type.
    /// `len` is the length of the allocated slice in T
    unsafe fn as_slice<T>(&self, len: usize) -> &[T] {
        std::slice::from_raw_parts(self.data.as_ptr().cast(), len)
    }

    /// Returns the `index`th element of type represented by info.
    unsafe fn elem_raw(&mut self, index: usize, info: &ComponentInfo) -> *mut u8 {
        self.data.as_ptr().offset((index * info.layout.size()) as _)
    }
}
