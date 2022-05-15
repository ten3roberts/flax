use std::{
    alloc::{alloc, dealloc, Layout},
    any::type_name,
    ptr::NonNull,
};

use crate::{entity::EntityLocation, util::SparseVec, Component, ComponentId, ComponentValue};

pub type ArchetypeId = u32;

#[derive(Debug, Clone, PartialEq)]
pub struct Archetype {
    component_map: Box<[usize]>,
    components: Box<[ComponentInfo]>,
    storage: Box<[Storage]>,
    len: usize,
}

impl Archetype {
    pub fn new(components: Vec<ComponentInfo>) -> Self {
        let max_component = components.iter().max_by_key(|v| v.id).unwrap();

        let mut component_map = vec![0; max_component.id.as_u64() as _].into_boxed_slice();
        let storage = components
            .iter()
            .enumerate()
            .map(|(i, component)| {
                component_map[component.id.as_u64() as usize] = i;
                Storage {
                    data: NonNull::dangling(),
                }
            })
            .collect();

        Self {
            len: 0,
            component_map,
            components: components.into_boxed_slice(),
            storage,
        }
    }

    pub fn storage_mut<T: ComponentValue>(
        &mut self,
        component: Component<T>,
    ) -> StorageBorrowMut<T> {
        let storage = self
            .storage
            .get(
                *self
                    .component_map
                    .get(component.id().as_u64() as usize)
                    .unwrap(),
            )
            .unwrap();

        // Type is guaranteed by `component_map`
        let data =
            unsafe { std::slice::from_raw_parts_mut(storage.data.as_ptr() as *mut T, self.len) };

        StorageBorrowMut {
            data,
            id: component.id(),
        }
    }

    pub fn storage<T: ComponentValue>(&self, component: Component<T>) -> StorageBorrow<T> {
        let storage = self
            .storage
            .get(
                *self
                    .component_map
                    .get(component.id().as_u64() as usize)
                    .unwrap(),
            )
            .unwrap();

        // Type is guaranteed by `component_map`
        let data =
            unsafe { std::slice::from_raw_parts(storage.data.as_ptr() as *const T, self.len) };

        StorageBorrow {
            data,
            id: component.id(),
        }
    }

    pub fn get<T: ComponentValue>(&self, entity: &EntityLocation, component: Component<T>) -> &T {
        let storage = self.storage(component);

        let value = &storage.data[entity.location];
        value
    }

    pub fn insert<T: ComponentValue>(&self, components: ComponentBuffer) -> usize {
        todo!()
    }
}

#[derive(Debug)]
pub struct ComponentBuffer {
    components: Vec<ComponentInfo>,
    /// Stores ComponentId => offset into data
    component_map: SparseVec<usize>,
    layout: Layout,
    data: NonNull<u8>,
    len: usize, // Number of meaningful bytes
}

impl ComponentBuffer {
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
            component_map: SparseVec::new(),
            data: NonNull::dangling(),
            len: 0,
            layout: Layout::from_size_align(0, 8).unwrap(),
        }
    }

    pub fn get_mut<T: ComponentValue>(&self, component: Component<T>) -> Option<&mut T> {
        let &offset = self.component_map.get(component.id().as_u64())?;

        Some(unsafe { &mut *self.data.as_ptr().offset(offset as _).cast() })
    }

    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Option<&T> {
        let &offset = self.component_map.get(component.id().as_u64())?;

        Some(unsafe { &*self.data.as_ptr().offset(offset as _).cast() })
    }

    pub fn insert<T: ComponentValue>(&mut self, component: Component<T>, value: T) {
        if let Some(&offset) = self.component_map.get(component.id().as_u64()) {
            unsafe {
                let ptr = self.data.as_ptr().offset(offset as _) as *mut T;
                *ptr = value;
            }
        } else {
            let layout = Layout::new::<T>();
            let offset = self.len + (self.len % layout.align());
            let new_len = offset + layout.size();
            // Reallocate if the current buffer cannot fit an additional
            // T+align bytes
            if new_len >= self.layout.size() {
                println!("Here");
                // Enforce alignment to be the strictest of all stored types
                let alignment = self.layout.align().max(layout.align());

                let new_layout =
                    Layout::from_size_align(new_len.next_power_of_two(), alignment).unwrap();

                unsafe {
                    // Don't realloc since  layout may change
                    let new_data = alloc(new_layout);

                    std::ptr::copy_nonoverlapping(self.data.as_ptr(), new_data, self.len);

                    if self.layout.size() != 0 {
                        dealloc(self.data.as_ptr(), self.layout)
                    }

                    self.data = NonNull::new(new_data).unwrap();
                }
                self.layout = new_layout;
            }

            eprintln!("pad: {}", self.len % layout.align());

            // Regardless, the bytes after `len` are allocated and
            // unoccupied
            unsafe {
                let ptr = self.data.as_ptr().offset(offset as _) as *mut T;
                *ptr = value;
            }
            assert_eq!(
                self.component_map.insert(component.id().as_u64(), offset),
                None
            );
            self.len = new_len;
        }
    }
}

impl Default for ComponentBuffer {
    fn default() -> Self {
        Self::new()
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
}

impl ComponentInfo {
    pub fn of<T: ComponentValue>(id: ComponentId) -> Self {
        Self {
            type_name: type_name::<T>(),
            layout: Layout::new::<T>(),
            id,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::component;

    use super::*;

    component! {
        a: i32,
        b: String,
        c: i16,
        d: f32,
    }

    #[test]
    pub fn component_buffer() {
        let mut buffer = ComponentBuffer::new();
        buffer.insert(a(), 7);
        buffer.insert(c(), 9);
        buffer.insert(b(), "Hello, World".to_string());

        assert_eq!(buffer.get(a()), Some(&7));
        assert_eq!(buffer.get(c()), Some(&9));
        assert_eq!(buffer.get(b()), Some(&"Hello, World".to_string()));
        assert_eq!(buffer.get(d()), None);
    }
}
