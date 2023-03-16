use core::alloc::Layout;
use core::mem;
use core::ptr::{self, NonNull};

use alloc::alloc::{dealloc, handle_alloc_error};
use alloc::collections::{btree_map, BTreeMap};
use itertools::Itertools;

use crate::{Component, ComponentInfo, ComponentValue};

type Offset = usize;

struct BufferEntry {
    drop_fn: unsafe fn(*mut u8),
    info: ComponentInfo,
}

/// A type erased bump allocator
/// Drops all remaining values on drop
pub(crate) struct BufferStorage {
    data: NonNull<u8>,
    cursor: usize,
    size: usize,
}

impl BufferStorage {
    fn new() -> Self {
        Self {
            data: NonNull::dangling(),
            cursor: 0,
            size: 0,
        }
    }

    /// Allocate space for a value with `layout`.
    /// Returns an offset into the internal data where a value of the compatible layout may be
    /// written.
    fn allocate(&mut self, layout: Layout) -> Offset {
        // Offset + the remaining padding to get the current offset up to an alignment boundary of `layout`.
        let new_offset = self.cursor + (layout.align() - self.cursor % layout.align());
        // The end of the allocated item
        let new_end = new_offset + layout.size();

        // Reallocate buffer if it is not large enough
        if new_end >= self.size && new_end != 0 {
            let new_size = new_end.next_power_of_two();

            {
                // Don't realloc since layout may change
                let new_layout = Layout::from_size_align(new_size, 1).unwrap();
                let old_layout = Layout::from_size_align(self.size, 1).unwrap();

                let new_data = if self.size == 0 {
                    unsafe { alloc::alloc::alloc(new_layout) }
                } else {
                    eprintln!("Reallocating to {new_size}");
                    unsafe { alloc::alloc::realloc(self.data.as_ptr(), old_layout, new_size) }
                };

                eprintln!("Got: {new_data:?}");

                let new_data = match NonNull::new(new_data) {
                    Some(v) => v,
                    None => handle_alloc_error(layout),
                };

                self.size = new_layout.size();
                self.data = new_data;
            }
        }

        self.cursor = new_end;
        new_offset
    }

    /// Moves the value out of the storage
    ///
    /// # Safety
    /// Multiple reads to the same offset is undefined as the value is moved.
    ///
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn take<T>(&mut self, offset: Offset) -> T {
        core::ptr::read(self.data.as_ptr().add(offset).cast::<T>())
    }

    /// Replaces the value at offset with `value`, returning the old value
    ///
    /// # Safety
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn replace<T>(&mut self, offset: Offset, value: T) -> T {
        let dst = self.data.as_ptr().add(offset).cast::<T>();

        mem::replace(unsafe { &mut *dst }, value)
    }

    /// Returns the value at offset as a reference to T
    /// # Safety
    /// The data at `offset` must be of type T and acquired from [`Self::allocate`]
    pub(crate) unsafe fn read<T>(&self, offset: Offset) -> &T {
        &*self.data.as_ptr().add(offset).cast::<T>()
    }

    pub(crate) unsafe fn at(&mut self, offset: Offset) -> *mut u8 {
        self.data.as_ptr().add(offset)
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
    pub(crate) unsafe fn write<T>(&mut self, offset: Offset, info: ComponentInfo, data: T) {
        let layout = Layout::new::<T>();
        let dst = self.data.as_ptr().add(offset).cast::<T>();

        assert_eq!(
            self.data.as_ptr() as usize % layout.align(),
            0,
            "Improper alignment"
        );

        assert_eq!(dst as usize % layout.align(), 0);

        core::ptr::write(dst, data);
    }

    /// Overwrites data at offset without reading or dropping the old value
    /// # Safety
    /// The existing data at offset is overwritten without calling drop on the contained value.
    /// The offset is must be allocated from [`Self::allocate`] with the layout of `T`
    pub(crate) unsafe fn write_dyn(&mut self, offset: Offset, info: ComponentInfo, data: *mut u8) {
        let dst = self.data.as_ptr().add(offset);
        let layout = info.layout();

        assert_eq!(
            self.data.as_ptr() as usize % layout.align(),
            0,
            "Improper alignment"
        );

        core::ptr::copy_nonoverlapping(data, dst, layout.size());
    }

    /// Resets the buffer, discarding the previously held data
    pub(crate) fn clear(&mut self) {
        self.cursor = 0;
    }

    /// Insert a new value into storage
    /// Is equivalent to an alloc followed by a write
    pub(crate) fn push<T>(&mut self, info: ComponentInfo, value: T) -> Offset {
        let offset = self.allocate(Layout::new::<T>());

        unsafe {
            self.write(offset, info, value);
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
        if self.size > 0 {
            let layout = Layout::from_size_align(self.size, 1).unwrap();
            unsafe { dealloc(self.data.as_ptr(), layout) }
        }
    }
}

/// Storage for components.
/// Can hold up to one if each component.
///
/// Used for gathering up an entity's components or inserting it.
///
/// This is a low level building block. Prefer [EntityBuilder](crate::EntityBuilder) or [CommandBuffer](crate::CommandBuffer) instead.
#[derive(Default)]
pub struct ComponentBuffer {
    entries: BTreeMap<ComponentInfo, Offset>,
    storage: BufferStorage,
}

impl core::fmt::Debug for ComponentBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.components().collect_vec())
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
        let &offset = self.entries.get(&component.info())?;

        unsafe { Some(self.storage.read_mut(offset)) }
    }

    /// Access a component from the buffer
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Option<&T> {
        let &offset = self.entries.get(&component.info())?;

        unsafe { Some(self.storage.read(offset)) }
    }

    /// Returns the components in the buffer
    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.entries.keys()
    }

    /// Remove a component from the component buffer
    pub fn remove<T: ComponentValue>(&mut self, component: Component<T>) -> Option<T> {
        let offset = self.entries.remove(&component.info())?;

        unsafe { Some(self.storage.take(offset)) }
    }

    /// Set a component in the component buffer
    pub fn set<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> Option<T> {
        let info = component.info();

        if let Some(&offset) = self.entries.get(&info) {
            unsafe { Some(self.storage.replace(offset, value)) }
        } else {
            let offset = self.storage.push(info, value);

            self.entries.insert(info, offset);

            None
        }
    }

    /// Set from a type erased component
    pub(crate) unsafe fn set_dyn(&mut self, info: ComponentInfo, value: *mut u8) {
        if let Some(&offset) = self.entries.get(&info) {
            let old_ptr = self.storage.at(offset);
            info.drop(old_ptr);

            ptr::copy_nonoverlapping(value, old_ptr, info.size());
        } else {
            let offset = self.storage.allocate(info.layout());

            self.storage.write_dyn(offset, info, value);

            self.entries.insert(info, offset);
        }
    }

    /// Pops a value from the buffer and returns a pointer to the value.
    ///
    /// # Safety
    /// The pointer is valid until the next write to the buffer
    pub(crate) fn pop(&mut self) -> Option<(ComponentInfo, *mut u8)> {
        let (info, offset) = self.entries.pop_first()?;
        let ptr = unsafe { self.storage.at(offset) };

        Some((info, ptr))
    }

    /// Drains the components from the buffer>
    ///
    /// The returned pointers must be manually dropped
    /// If the returned iterator is dropped before being fully consumed, the
    /// remaining values will be safely dropped.
    pub(crate) fn drain(&mut self) -> ComponentBufferIter {
        ComponentBufferIter {
            entries: &mut self.entries,
            storage: &mut self.storage,
        }
    }
}

pub(crate) struct ComponentBufferIter<'a> {
    entries: &'a mut BTreeMap<ComponentInfo, Offset>,
    storage: &'a mut BufferStorage,
}

impl<'a> Iterator for ComponentBufferIter<'a> {
    type Item = (ComponentInfo, *mut u8);

    fn next(&mut self) -> Option<Self::Item> {
        let (info, offset) = self.entries.pop_first()?;

        unsafe {
            let data = self.storage.at(offset);
            Some((info, data))
        }
    }
}

impl Drop for ComponentBuffer {
    fn drop(&mut self) {
        for (info, &offset) in &self.entries {
            unsafe {
                let ptr = self.storage.at(offset);
                info.drop(ptr);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use core::mem;

    use alloc::{string::String, sync::Arc};

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
        let shared: Arc<String> = Arc::new("abc".into());
        let mut buffer = ComponentBuffer::new();
        buffer.set(a(), 7);
        buffer.set(c(), 9);
        buffer.set(b(), "Hello, World".into());
        buffer.set(e(), [5.0; 100]);

        buffer.set(f(), shared.clone());

        assert_eq!(buffer.get(a()), Some(&7));
        assert_eq!(buffer.get(c()), Some(&9));
        assert_eq!(buffer.get(b()), Some(&"Hello, World".into()));
        assert_eq!(buffer.get(d()), None);
        assert_eq!(buffer.get(e()), Some(&[5.0; 100]));

        drop(buffer);

        assert_eq!(Arc::strong_count(&shared), 1);
    }

    #[test]
    pub fn component_buffer_reinsert() {
        let mut buffer = ComponentBuffer::new();

        let shared: Arc<String> = Arc::new("abc".into());
        let shared_2: Arc<String> = Arc::new("abc".into());
        buffer.set(f(), shared.clone());
        buffer.set(f(), shared_2.clone());

        assert_eq!(Arc::strong_count(&shared), 1);
        assert_eq!(Arc::strong_count(&shared_2), 2);
    }

    #[test]
    pub fn component_buffer_reinsert_dyn() {
        let mut buffer = ComponentBuffer::new();

        let shared: Arc<String> = Arc::new("abc".into());
        let shared_2: Arc<String> = Arc::new("abc".into());
        unsafe {
            let mut shared = shared.clone();
            buffer.set_dyn(f().info(), &mut shared as *mut _ as *mut u8);
            mem::forget(shared)
        }

        unsafe {
            let mut shared = shared_2.clone();
            buffer.set_dyn(f().info(), &mut shared as *mut _ as *mut u8);
            mem::forget(shared)
        }

        assert_eq!(Arc::strong_count(&shared), 1);
        assert_eq!(Arc::strong_count(&shared_2), 2);
    }
}
