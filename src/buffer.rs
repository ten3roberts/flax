use core::alloc::Layout;
use core::mem::{self, align_of};
use core::ptr::{self, NonNull};

use alloc::alloc::{dealloc, handle_alloc_error, realloc};
use alloc::collections::BTreeMap;

use crate::format::MissingDebug;
use crate::metadata::debuggable;
use crate::{metadata, Component, ComponentInfo, ComponentKey, ComponentValue, Entity};

type Offset = usize;

/// A type erased bump allocator
/// Does not handle dropping of the values
pub(crate) struct BufferStorage {
    data: NonNull<u8>,
    cursor: usize,
    layout: Layout,
}

impl BufferStorage {
    fn new() -> Self {
        Self {
            data: NonNull::dangling(),
            cursor: 0,
            layout: Layout::from_size_align(0, align_of::<u8>()).unwrap(),
        }
    }

    /// Allocate space for a value with `layout`.
    /// Returns an offset into the internal data where a value of the compatible layout may be
    /// written.
    fn allocate(&mut self, item_layout: Layout) -> Offset {
        // Offset + the remaining padding to get the current offset up to an alignment boundary of `layout`.
        let new_offset = self.cursor + (item_layout.align() - self.cursor % item_layout.align());
        // The end of the allocated item
        let new_end = new_offset + item_layout.size();

        // Reallocate buffer if it is not large enough
        if (new_end >= self.layout.size() && new_end != 0)
            || self.layout.align() < item_layout.align()
        {
            let new_size = new_end.next_power_of_two();

            // Don't realloc since layout may change
            let new_align = self.layout.align().max(item_layout.align());
            let new_layout = Layout::from_size_align(new_size, new_align).unwrap();

            let new_data = if self.layout.size() == 0 {
                match NonNull::new(unsafe { alloc::alloc::alloc(new_layout) }) {
                    Some(v) => v,
                    None => handle_alloc_error(new_layout),
                }
            } else if new_align != self.layout.align() {
                unsafe {
                    let old_ptr = self.data.as_ptr();
                    let new_ptr = match NonNull::new(alloc::alloc::alloc(new_layout)) {
                        Some(v) => v,
                        None => handle_alloc_error(new_layout),
                    };
                    ptr::copy_nonoverlapping(old_ptr, new_ptr.as_ptr(), self.cursor);
                    dealloc(old_ptr, self.layout);
                    new_ptr
                }
            } else {
                unsafe {
                    match NonNull::new(realloc(self.data.as_ptr(), self.layout, new_size)) {
                        Some(v) => v,
                        None => alloc::alloc::handle_alloc_error(self.layout),
                    }
                }
            };

            self.layout = new_layout;
            self.data = new_data;
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

    pub(crate) unsafe fn at_mut(&mut self, offset: Offset) -> *mut u8 {
        self.data.as_ptr().add(offset)
    }

    pub(crate) unsafe fn at(&self, offset: Offset) -> *const u8 {
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
    pub(crate) unsafe fn write<T>(&mut self, offset: Offset, data: T) {
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
    #[inline(always)]
    pub(crate) fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Insert a new value into storage
    /// Is equivalent to an alloc followed by a write
    pub(crate) fn push<T>(&mut self, value: T) -> Offset {
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
        if self.layout.size() > 0 {
            unsafe { dealloc(self.data.as_ptr(), self.layout) }
        }
    }
}

/// Storage for components.
/// Can hold up to one of each component.
///
/// Used for gathering up an entity's components or inserting it.
///
/// This is a low level building block. Prefer [EntityBuilder](crate::EntityBuilder) or [CommandBuffer](crate::CommandBuffer) instead.
#[derive(Default)]
pub struct ComponentBuffer {
    entries: BTreeMap<ComponentKey, (ComponentInfo, Offset)>,
    storage: BufferStorage,
}

impl core::fmt::Debug for ComponentBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut s = f.debug_map();

        for &(info, offset) in self.entries.values() {
            let debugger = info.meta_ref().get(debuggable());
            if let Some(debugger) = debugger {
                unsafe {
                    let ptr = self.storage.at(offset);
                    s.entry(&info.name(), debugger.debug_ptr(&ptr));
                }
            } else {
                s.entry(&info.name(), &MissingDebug);
            }
        }

        s.finish()
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
        let &(_, offset) = self.entries.get(&component.key())?;

        unsafe { Some(self.storage.read_mut(offset)) }
    }

    /// Access a component from the buffer
    pub fn get<T: ComponentValue>(&self, component: Component<T>) -> Option<&T> {
        let &(_, offset) = self.entries.get(&component.key())?;

        unsafe { Some(self.storage.read(offset)) }
    }

    /// Returns true if the buffer contains the given component
    pub fn has<T: ComponentValue>(&self, component: Component<T>) -> bool {
        self.entries.contains_key(&component.key())
    }

    /// Returns the components in the buffer
    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.entries.values().map(|v| &v.0)
    }

    /// Remove a component from the component buffer
    pub fn remove<T: ComponentValue>(&mut self, component: Component<T>) -> Option<T> {
        let (_, offset) = self.entries.remove(&component.key())?;

        unsafe { Some(self.storage.take(offset)) }
    }

    /// Set a component in the component buffer
    pub fn set<T: ComponentValue>(&mut self, component: Component<T>, value: T) -> Option<T> {
        let info = component.info();

        if let Some(&(_, offset)) = self.entries.get(&info.key()) {
            unsafe { Some(self.storage.replace(offset, value)) }
        } else {
            if info.key().is_relation() && info.meta_ref().has(metadata::exclusive()) {
                self.drain_relations_like(info.key.id());
            }

            let offset = self.storage.push(value);

            self.entries.insert(info.key(), (info, offset));

            None
        }
    }

    pub(crate) fn drain_relations_like(&mut self, relation: Entity) {
        let start = ComponentKey::new(relation, Some(Entity::MIN));
        let end = ComponentKey::new(relation, Some(Entity::MAX));

        while let Some((&key, _)) = self.entries.range(start..=end).next() {
            let (info, offset) = self.entries.remove(&key).unwrap();
            unsafe {
                let ptr = self.storage.at_mut(offset);
                info.drop(ptr);
            }
        }
    }

    /// Set from a type erased component
    pub(crate) unsafe fn set_dyn(&mut self, info: ComponentInfo, value: *mut u8) {
        if let Some(&(_, offset)) = self.entries.get(&info.key()) {
            let old_ptr = self.storage.at_mut(offset);
            info.drop(old_ptr);

            ptr::copy_nonoverlapping(value, old_ptr, info.size());
        } else {
            if info.key().is_relation() && info.meta_ref().has(metadata::exclusive()) {
                self.drain_relations_like(info.key.id());
            }

            let offset = self.storage.allocate(info.layout());

            self.storage.write_dyn(offset, info, value);

            self.entries.insert(info.key(), (info, offset));
        }
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

    #[inline]
    /// Returns the number of component in the buffer
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    #[inline]
    /// Returns `true` if the buffer contains no components
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Retains only the components specified by the predicate
    /// If the closure returns true the element is removed and **not** dropped at the end of
    /// collection
    /// # Safety
    /// If the passed closure returns *false* the element is considered moved and shall be handled by
    /// the caller.
    pub(crate) unsafe fn retain(&mut self, mut f: impl FnMut(ComponentInfo, *mut ()) -> bool) {
        self.entries.retain(|_, (info, offset)| {
            let ptr = unsafe { self.storage.at_mut(*offset) };
            f(*info, ptr as *mut ())
        })
    }
}

pub(crate) struct ComponentBufferIter<'a> {
    entries: &'a mut BTreeMap<ComponentKey, (ComponentInfo, Offset)>,
    storage: &'a mut BufferStorage,
}

impl<'a> Iterator for ComponentBufferIter<'a> {
    type Item = (ComponentInfo, *mut u8);

    fn next(&mut self) -> Option<Self::Item> {
        let (_, (info, offset)) = self.entries.pop_first()?;

        unsafe {
            let data = self.storage.at_mut(offset);
            Some((info, data))
        }
    }
}

impl Drop for ComponentBuffer {
    fn drop(&mut self) {
        for &(info, offset) in self.entries.values() {
            unsafe {
                let ptr = self.storage.at_mut(offset);
                info.drop(ptr);
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct MultiComponentBuffer {
    storage: BufferStorage,
    drops: BTreeMap<Offset, unsafe fn(*mut u8)>,
}

impl MultiComponentBuffer {
    /// Push a new value into the buffer
    pub fn push<T: ComponentValue>(&mut self, value: T) -> Offset {
        let offset = self.storage.push(value);
        let old = self
            .drops
            .insert(offset, unsafe { |ptr| ptr.cast::<T>().drop_in_place() });

        assert!(old.is_none());
        offset
    }

    pub unsafe fn take_dyn(&mut self, offset: Offset) -> *mut u8 {
        self.drops.remove(&offset).unwrap();
        self.storage.at_mut(offset)
    }

    pub fn clear(&mut self) {
        for (&offset, drop) in &mut self.drops {
            unsafe {
                let ptr = self.storage.at_mut(offset);
                (drop)(ptr)
            }
        }
        self.drops.clear();
        self.storage.reset();
    }
}

impl Drop for MultiComponentBuffer {
    fn drop(&mut self) {
        self.clear();
    }
}

unsafe impl Send for MultiComponentBuffer {}
unsafe impl Sync for MultiComponentBuffer {}

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

    #[test]
    fn multi_component_buffer() {
        let mut buffer = MultiComponentBuffer::default();
        let shared = Arc::new(4);

        let a = buffer.push(9i32);
        let b = buffer.push(String::from("Hello, there"));
        let _c = buffer.push(shared.clone());
        let d = buffer.push(shared.clone());

        unsafe {
            assert_eq!(buffer.take_dyn(b).cast::<String>().read(), "Hello, there");
            assert_eq!(buffer.take_dyn(a).cast::<i32>().read(), 9);
            assert_eq!(buffer.take_dyn(d).cast::<Arc<i32>>().read(), shared);
        }
        drop(buffer);

        assert_eq!(Arc::strong_count(&shared), 1);
    }
}
