use std::{
    alloc::{alloc, dealloc, Layout},
    ptr::NonNull,
};

use crate::{util::SparseVec, Component, ComponentBuffer, ComponentId, ComponentValue, Entity};

pub type ArchetypeId = u32;
pub type Slot = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct Archetype {
    component_map: Box<[usize]>,
    components: Box<[ComponentInfo]>,
    /// Slot to entity id
    entities: Box<[Option<Entity>]>,
    storage: Box<[Storage]>,
    // Number of entities in the archetype
    len: usize,
    // Number of slots
    cap: usize,

    // ComponentId => ArchetypeId
    // If the key is an existing component, it means it is a backwards edge
    edges: SparseVec<ArchetypeId>,
}

impl Archetype {
    pub fn empty() -> Self {
        Self {
            component_map: Box::new([]),
            components: Box::new([]),
            storage: Box::new([]),
            len: 0,
            cap: 0,
            edges: SparseVec::new(),
            entities: Box::new([]),
        }
    }

    /// Create a new archetype.
    /// Assumes `components` are sorted by id.
    pub fn new(components: Vec<ComponentInfo>) -> Self {
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
            edges: SparseVec::new(),
            entities: Box::new([]),
        }
    }

    /// Returns true if the archtype has `component`
    pub fn has(&self, component: ComponentId) -> bool {
        self.component_map
            .get(component.as_raw() as usize)
            .unwrap_or(&0)
            != &0
    }

    pub fn edge_to(&self, component: ComponentId) -> Option<ArchetypeId> {
        self.edges.get(component.as_raw()).copied()
    }

    pub fn add_edge_to(
        &mut self,
        dst: &mut Archetype,
        dst_id: ArchetypeId,
        src_id: ArchetypeId,
        component: ComponentId,
    ) {
        assert!(self.edges.insert(component.as_raw(), dst_id).is_none());
        assert!(dst.edges.insert(component.as_raw(), src_id).is_none());
    }

    pub fn storage_mut<T: ComponentValue>(
        &mut self,
        component: Component<T>,
    ) -> Option<StorageBorrowMut<T>> {
        let index = *self
            .component_map
            .get(component.id().as_raw() as usize)
            .unwrap_or(&0);

        if index == 0 {
            None
        } else {
            let storage = &mut self.storage[(index - 1)];

            // Type is guaranteed by `component_map`
            let data = unsafe {
                std::slice::from_raw_parts_mut(storage.data.as_ptr() as *mut T, self.len)
            };

            Some(StorageBorrowMut {
                data,
                id: component.id(),
            })
        }
    }

    pub fn storage<T: ComponentValue>(&self, component: Component<T>) -> Option<StorageBorrow<T>> {
        let index = *self
            .component_map
            .get(component.id().as_raw() as usize)
            .unwrap_or(&0);

        if index == 0 {
            None
        } else {
            let storage = &self.storage[(index - 1)];

            // Type is guaranteed by `component_map`
            let data =
                unsafe { std::slice::from_raw_parts(storage.data.as_ptr() as *const T, self.len) };

            Some(StorageBorrow {
                data,
                id: component.id(),
            })
        }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get_mut<T: ComponentValue>(
        &mut self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<&mut T> {
        let storage = self.storage_mut(component)?;

        Some(&mut storage.data[slot])
    }

    fn storage_raw(&mut self, id: ComponentId) -> Option<&Storage> {
        let index = *self.component_map.get(id.as_raw() as usize).unwrap_or(&0);

        if index == 0 {
            None
        } else {
            Some(&self.storage[index - 1])
        }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get<T: ComponentValue>(&self, slot: Slot, component: Component<T>) -> Option<&T> {
        let storage = self.storage(component)?;

        Some(&storage.data[slot])
    }

    /// Insert a new entity into the archetype.
    /// The components must match exactly.
    ///
    /// Returns the index of the entity
    /// Entity must not exist in archetype
    pub fn insert(&mut self, id: Entity, components: &mut ComponentBuffer) -> Slot {
        let slot = unsafe { self.allocate(id) };
        eprintln!("Inserting components into {slot}");
        unsafe {
            for (component, src) in components.take_all() {
                let storage = self.storage_raw(component.id).unwrap();
                std::ptr::copy_nonoverlapping(
                    src,
                    storage.at(&component, slot),
                    component.layout.size(),
                );
            }
        }

        slot
    }

    /// Allocated space for a new slot.
    /// # Safety
    /// All components of slot are uninitialized. `pub_dyn` needs to be called for
    /// all components in archetype.
    pub unsafe fn allocate(&mut self, id: Entity) -> Slot {
        self.reserve(1);

        let slot = self.len;

        self.len += 1;
        self.entities[slot] = Some(id);

        slot
    }

    /// Put a typeerased component info a slot.
    /// `src` shall be considered moved.
    /// `component` must match the type of data.
    /// Must be called only **ONCE**. Returns Err(src) if move was unsuccessful
    pub unsafe fn put_dyn(
        &mut self,
        slot: Slot,
        component: &ComponentInfo,
        src: *mut u8,
    ) -> Result<(), *mut u8> {
        let index = *self
            .component_map
            .get(component.id.as_raw() as usize)
            .unwrap_or(&0);

        if index == 0 {
            return Err(src);
        }

        let storage = &mut self.storage[(index - 1)];

        let dst = storage.at(component, slot);
        std::ptr::copy_nonoverlapping(src, dst, component.layout.size());

        Ok(())
    }

    /// Move all components in `slot` to archetype of `dst`. The components not
    /// in self will be left uninitialized.
    /// `dst.put_dyn` must be called immediately after for each missing
    /// component.
    ///
    /// Returns the slot in dst and entity which was moved into current `slot`, if any.
    pub unsafe fn move_to(&mut self, dst: &mut Self, slot: Slot) -> (Slot, Option<Entity>) {
        let entity = self.entity(slot).expect("Invalid entity");
        let dst_slot = dst.allocate(entity);
        let last = self.len - 1;

        for (storage, component) in self.storage.iter_mut().zip(self.components.iter()) {
            let src = storage.at(component, slot);
            match dst.put_dyn(dst_slot, component, src) {
                Ok(()) => {}
                Err(p) => {
                    eprintln!("Dropping component {component:?} in move");
                    (component.drop)(p);
                }
            };

            // Move back in to fill the gap
            if slot != last {
                let dst = storage.at(component, last);
                std::ptr::copy_nonoverlapping(src, dst, component.layout.size());
            }
        }

        self.len -= 1;

        if slot != last {
            self.entities[slot] = self.entities[last];
            (
                dst_slot,
                Some(std::mem::take(&mut self.entities[last]).expect("Invalid entity at last pos")),
            )
        } else {
            (dst_slot, None)
        }
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

        // Copy over entity ids
        let mut new_entities = vec![None; new_cap].into_boxed_slice();
        new_entities[0..self.len].copy_from_slice(&self.entities[0..self.len]);
        self.entities = new_entities;
        self.cap = new_cap;
    }

    pub fn entity(&self, slot: Slot) -> Option<Entity> {
        self.entities[slot]
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

    /// Get a reference to the archetype's components.
    #[must_use]
    pub fn components(&self) -> &[ComponentInfo] {
        self.components.as_ref()
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
    pub(crate) name: &'static str,
    pub(crate) layout: Layout,
    pub(crate) id: ComponentId,
    pub(crate) drop: unsafe fn(*mut u8),
}

impl ComponentInfo {
    pub fn of<T: ComponentValue>(component: Component<T>) -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }
        Self {
            drop: drop_ptr::<T>,
            name: component.name(),
            layout: Layout::new::<T>(),
            id: component.id(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{component, ComponentBuffer};

    use super::*;
    use std::num::NonZeroU32;

    component! {
        a: i32,
        b: String,
        c: Arc<String>,
    }

    #[test]
    pub fn test_archetype() {
        let mut arch = Archetype::new(vec![
            ComponentInfo::of(a()),
            ComponentInfo::of(b()),
            ComponentInfo::of(c()),
        ]);

        let shared = Arc::new("abc".to_string());

        let mut buffer = ComponentBuffer::new();
        buffer.insert(a(), 7);
        buffer.insert(b(), "Foo".to_string());
        buffer.insert(c(), shared.clone());

        let id = Entity::from_parts(NonZeroU32::new(6).unwrap(), 2);
        let id_2 = Entity::from_parts(NonZeroU32::new(7).unwrap(), 2);
        let slot = arch.insert(id, &mut buffer);
        eprintln!("Slot: {slot}");

        // Reuse buffer and insert again
        buffer.insert(a(), 9);
        buffer.insert(b(), "Bar".to_string());
        buffer.insert(c(), shared.clone());

        let slot_2 = arch.insert(id_2, &mut buffer);

        assert_eq!(slot, 0);
        assert_eq!(arch.get(slot, a()), Some(&7));
        assert_eq!(arch.get(slot, b()), Some(&"Foo".to_string()));
        assert_eq!(arch.get(slot_2, b()), Some(&"Bar".to_string()));

        arch.get_mut(slot, b())
            .unwrap()
            .push_str(&"Bar".to_string());

        assert_eq!(arch.get(slot, b()), Some(&"FooBar".to_string()));
        assert_eq!(arch.entity(slot), Some(id));
        assert_eq!(arch.entity(slot_2), Some(id_2));

        drop(arch);

        assert_eq!(Arc::strong_count(&shared), 1);
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
        self.data.as_ptr().add(index * info.layout.size())
    }

    unsafe fn at(&self, component: &ComponentInfo, slot: Slot) -> *mut u8 {
        self.data.as_ptr().add(component.layout.size() * slot)
    }
}
