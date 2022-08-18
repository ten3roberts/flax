use std::alloc::{alloc, handle_alloc_error};
use std::collections::{btree_map, BTreeMap, HashMap};
use std::mem;
use std::{
    alloc::{dealloc, Layout},
    ptr::NonNull,
};

use crate::ComponentId;
use crate::{archetype::ComponentInfo, Component, ComponentValue};

type Offset = usize;

#[derive(Debug, Clone)]
/// A type erased allocator
/// Drops all remaining values on drop
pub(crate) struct BufferStorage {
    data: NonNull<u8>,
    cursor: usize,
    layout: Layout,
    drops: HashMap<Offset, unsafe fn(*mut u8)>,
}

impl BufferStorage {
    fn new() -> Self {
        Self {
            data: NonNull::dangling(),
            cursor: 0,
            layout: Layout::from_size_align(0, 2).unwrap(),
            drops: HashMap::new(),
        }
    }

    /// Allocate space for a value with `layout`.
    /// Returns an offset into the internal data where a value of the compatible layout may lay.
    pub(crate) fn allocate(&mut self, layout: Layout) -> Offset {
        // Offset + the remaining padding to get the current offset up to an alignment boundary of `layout`.
        let new_offset = self.cursor + (layout.align() - self.cursor % layout.align());
        let new_len = new_offset + layout.size();

        if new_len >= self.layout.size() || layout.align() > self.layout.align() && new_len != 0 {
            eprintln!("Reallocating {} => {}", self.layout.size(), new_len);
            let align = self.layout.align().max(layout.align());
            let new_layout = Layout::from_size_align(new_len.next_power_of_two(), align).unwrap();

            unsafe {
                // Don't realloc since layout may change
                let new_data = alloc(new_layout);

                let new_data = match NonNull::new(new_data) {
                    Some(v) => v,
                    None => handle_alloc_error(layout),
                };

                if self.layout.size() > 0 {
                    std::ptr::copy_nonoverlapping(
                        self.data.as_ptr(),
                        new_data.as_ptr(),
                        self.cursor,
                    );
                    dealloc(self.data.as_ptr(), self.layout)
                }

                self.data = new_data;
            }
            self.layout = new_layout;
        }

        self.cursor = new_len;
        new_offset
    }

    /// Moves the value out of the storage
    /// # Safety
    /// The data at offset is unchanged.
    /// Reads to the same offset is undefined as the value has moved out.
    ///
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn take<T>(&mut self, offset: Offset) -> T {
        let data = std::ptr::read(self.data.as_ptr().add(offset).cast::<T>());
        if self.drops.remove(&offset).is_none() {
            panic!("Attempt to take non existent value");
        }

        data
    }

    /// Moves the value out of the storage
    /// # Safety
    /// The data at offset is unchanged.
    /// Reads to the same offset is undefined as the value has moved out.
    ///
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn take_dyn(&mut self, offset: Offset) -> *mut u8 {
        let data = self.data.as_ptr().add(offset);
        if self.drops.remove(&offset).is_none() {
            panic!("Attempt to take non existent value");
        }

        data
    }

    /// Swaps the value at offset with `value`, returning the old value
    ///
    /// # Safety
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn swap<T>(&mut self, offset: Offset, value: T) -> T {
        let prev = self.take(offset);
        self.write(offset, value);
        prev
    }

    /// Returns the value at offset as a reference to T
    /// # Safety
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn read<T>(&self, offset: Offset) -> &T {
        &*self.data.as_ptr().add(offset).cast::<T>()
    }

    /// Returns the value at offset as a reference to T
    /// # Safety
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn read_mut<T>(&mut self, offset: Offset) -> &mut T {
        &mut *self.data.as_ptr().add(offset).cast::<T>()
    }

    /// Overwrites data at offset without reading or dropping the old value
    /// # Safety
    /// The existing data at offset is overwritten without calling drop on the contained value.
    /// The offset is must be allocated from [`Self::allocate`] with the layout of `T`
    pub(crate) unsafe fn write<T>(&mut self, offset: Offset, data: T) {
        let layout = Layout::new::<T>();
        let dst = self.data.as_ptr().add(offset).cast::<T>();
        assert_eq!(
            self.data.as_ptr() as usize % layout.align(),
            0,
            "Improper alignment"
        );

        assert_eq!(dst as usize % layout.align(), 0);

        std::ptr::write(dst, data);

        // Add a function to drop this stored value
        self.drops
            .insert(offset, |ptr| std::ptr::drop_in_place(ptr.cast::<T>()));
    }

    /// Drops all values stored inside while keeping the allocation
    pub(crate) fn clear(&mut self) {
        let drops = std::mem::take(&mut self.drops);
        for (offset, drop_func) in drops {
            unsafe {
                let ptr = self.data.as_ptr().add(offset);
                (drop_func)(ptr);
            }
        }

        self.cursor = 0;
    }

    /// Insert a new value into storage
    /// Is equivalent to an alloc followed by a write
    pub(crate) fn insert<T>(&mut self, value: T) -> Offset {
        let offset = self.allocate(Layout::new::<T>());

        unsafe {
            self.write(offset, value);
        }

        offset
    }
}

impl Default for BufferStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BufferStorage {
    fn drop(&mut self) {
        self.clear();
        if self.layout.size() > 0 {
            unsafe { dealloc(self.data.as_ptr(), self.layout) }
        }
    }
}

/// Storage for components.
/// Can hold up to one if each component.
///
/// Used for gathering up an entity's components or inserting it.
#[derive(Default)]
pub struct ComponentBuffer {
    components: BTreeMap<ComponentId, (Offset, ComponentInfo)>,
    storage: BufferStorage,
}

impl<'a> IntoIterator for &'a mut ComponentBuffer {
    type Item = (ComponentInfo, *mut u8);

    type IntoIter = ComponentBufferIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.take_all()
    }
}

impl std::fmt::Debug for ComponentBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.components().map(|v| v.name()))
            .finish()
    }
}

/// Since all components are Send + Sync, the componentbuffer is as well
unsafe impl Send for ComponentBuffer {}
unsafe impl Sync for ComponentBuffer {}

impl ComponentBuffer {
    /// Creates a new component buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Mutably access a component from the buffer
    pub fn get_mut<T: ComponentValue>(&mut self, component: Component<T>) -> Option<&mut T> {
        let &(offset, _) = self.components.get(&component.id())?;

        unsafe { Some(self.storage.read_mut(offset)) }
    }

    /// Access a component from the buffer
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Option<&T> {
        let &(offset, _) = self.components.get(&component.id())?;

        unsafe { Some(self.storage.read(offset)) }
    }

    /// Returns the component in the buffer
    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.components.values().map(|v| &v.1)
    }

    pub(crate) fn components_mut(&mut self) -> impl Iterator<Item = &mut ComponentInfo> {
        self.components.values_mut().map(|v| &mut v.1)
    }

    /// Remove a component from the component buffer
    pub fn remove<T: ComponentValue>(&mut self, component: Component<T>) -> Option<T> {
        let (offset, _) = self.components.remove(&component.id())?;

        unsafe { Some(self.storage.take(offset)) }
    }

    /// Set a component in the component buffer
    pub fn set<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> Option<T> {
        self.set_dyn(component.info(), value)
    }

    /// Set from a type erased component
    pub fn set_dyn<T: ComponentValue>(&mut self, component: ComponentInfo, value: T) -> Option<T> {
        if let Some(&(offset, _)) = self.components.get(&component.id()) {
            unsafe { Some(self.storage.swap(offset, value)) }
        } else {
            let offset = self.storage.insert(value);

            self.components.insert(component.id(), (offset, component));

            None
        }
    }

    /// Take all components for the buffer.
    /// The yielded pointers needs to be dropped manually.
    /// If the returned iterator is dropped before being fully consumed, the
    /// remaining values will be safely dropped.
    pub fn take_all(&mut self) -> ComponentBufferIter {
        let components = &mut self.components;
        let storage = &mut self.storage;
        ComponentBufferIter {
            components: mem::take(components).into_iter(),
            storage,
        }
    }
}

/// Iterate all items in the component buffer
pub struct ComponentBufferIter<'a> {
    components: btree_map::IntoIter<ComponentId, (Offset, ComponentInfo)>,
    storage: &'a mut BufferStorage,
}

impl<'a> Iterator for ComponentBufferIter<'a> {
    type Item = (ComponentInfo, *mut u8);

    fn next(&mut self) -> Option<Self::Item> {
        let (_, (offset, component)) = self.components.next()?;
        unsafe {
            let data = self.storage.take_dyn(offset);
            Some((component, data))
        }
    }
}

impl<'a> Drop for ComponentBufferIter<'a> {
    fn drop(&mut self) {
        self.storage.clear();
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
        buffer.set(a(), 7);
        buffer.set(c(), 9);
        buffer.set(b(), "Hello, World".to_string());
        buffer.set(e(), [5.0; 100]);

        buffer.set(f(), shared.clone());

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
        buffer.set(f(), shared.clone());
        buffer.set(f(), shared_2.clone());

        assert_eq!(Arc::strong_count(&shared), 1);
        assert_eq!(Arc::strong_count(&shared_2), 2);
    }
}
