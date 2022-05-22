use std::{
    alloc::{alloc, dealloc, Layout},
    collections::BTreeMap,
    ops::Range,
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{Component, ComponentBuffer, ComponentId, ComponentValue, Entity};

pub type ArchetypeId = u32;
pub type Slot = usize;

#[derive(Debug)]
pub struct Archetype {
    storage: BTreeMap<Entity, Storage>,
    /// Slot to entity id
    entities: Box<[Option<Entity>]>,
    // Number of entities in the archetype
    len: usize,
    // Number of slots
    cap: usize,

    // ComponentId => ArchetypeId
    // If the key is an existing component, it means it is a backwards edge
    edges: BTreeMap<Entity, ArchetypeId>,
}

impl Archetype {
    pub fn empty() -> Self {
        Self {
            storage: BTreeMap::new(),
            len: 0,
            cap: 0,
            edges: BTreeMap::new(),
            entities: Box::new([]),
        }
    }

    /// Create a new archetype.
    /// Assumes `components` are sorted by id.
    pub fn new(components: impl IntoIterator<Item = ComponentInfo>) -> Self {
        let storage = components
            .into_iter()
            .map(|component| {
                (
                    component.id,
                    Storage {
                        data: AtomicRefCell::new(NonNull::dangling()),
                        component,
                    },
                )
            })
            .collect();

        Self {
            len: 0,
            cap: 0,
            storage,
            edges: BTreeMap::new(),
            entities: Box::new([]),
        }
    }

    pub fn slots(&self) -> Range<Slot> {
        0..self.len
    }

    /// Returns true if the archtype has `component`
    pub fn has(&self, component: ComponentId) -> bool {
        self.storage.get(&component).is_some()
    }

    pub fn edge_to(&self, component: ComponentId) -> Option<ArchetypeId> {
        self.edges.get(&component).copied()
    }

    pub fn add_edge_to(
        &mut self,
        dst: &mut Archetype,
        dst_id: ArchetypeId,
        src_id: ArchetypeId,
        component: ComponentId,
    ) {
        self.edges.insert(component, dst_id);
        dst.edges.insert(component, src_id);
    }

    pub fn storage_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> Option<StorageBorrowMut<T>> {
        let len = self.len;
        let storage = self.storage.get(&component.id())?;

        // Type is guaranteed by `map`
        let data = storage.data.borrow_mut();
        let data = AtomicRefMut::map(data, |v| unsafe {
            std::slice::from_raw_parts_mut(v.as_ptr().cast::<T>(), len)
        });

        Some(StorageBorrowMut {
            data,
            id: component.id(),
        })
    }

    #[inline]
    pub fn storage<T: ComponentValue>(&self, component: Component<T>) -> Option<StorageBorrow<T>> {
        let storage = self.storage.get(&component.id())?;
        // Type is guaranteed by `map`
        let data = storage.data.borrow();
        let data = AtomicRef::map(data, |v| unsafe {
            std::slice::from_raw_parts_mut(v.as_ptr().cast::<T>(), self.len)
        });

        Some(StorageBorrow {
            data,
            id: component.id(),
        })
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get_mut<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRefMut<T>> {
        let storage = unsafe { self.storage_raw(component.id())? };

        if slot < self.len {
            Some(AtomicRefMut::map(storage.data.borrow_mut(), |v| unsafe {
                &mut *(v.as_ptr().cast::<T>().add(slot))
            }))
        } else {
            None
        }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        let storage = unsafe { self.storage_raw(component.id())? };

        if slot < self.len {
            Some(AtomicRef::map(storage.data.borrow(), |v| unsafe {
                &*(v.as_ptr().cast::<T>().add(slot))
            }))
        } else {
            None
        }
    }

    pub(crate) unsafe fn storage_raw_mut(&mut self, id: ComponentId) -> Option<&mut Storage> {
        self.storage.get_mut(&id)
    }

    pub(crate) unsafe fn storage_raw(&self, id: ComponentId) -> Option<&Storage> {
        self.storage.get(&id)
    }

    /// Insert a new entity into the archetype.
    /// The components must match exactly.
    ///
    /// Returns the index of the entity
    /// Entity must not exist in archetype
    pub fn insert(&mut self, id: Entity, components: &mut ComponentBuffer) -> Slot {
        let slot = unsafe { self.allocate(id) };
        unsafe {
            for (component, src) in components.take_all() {
                let storage = self.storage_raw_mut(component.id).unwrap();
                std::ptr::copy_nonoverlapping(
                    src,
                    storage
                        .data
                        .get_mut()
                        .as_ptr()
                        .add(component.layout.size() * slot),
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
        let storage = self.storage.get_mut(&component.id).ok_or(src)?;

        assert_eq!(component, &storage.component);
        let dst = storage.at_mut(slot);
        std::ptr::copy_nonoverlapping(src, dst, component.layout.size());

        Ok(())
    }

    /// Move all components in `slot` to archetype of `dst`. The components not
    /// in self will be left uninitialized.
    /// `dst.put_dyn` must be called immediately after for each missing
    /// component.
    ///
    /// Returns the slot in dst and entity which was moved into current `slot`, if any.
    pub unsafe fn move_to(
        &mut self,
        dst: &mut Self,
        slot: Slot,
        mut on_drop: impl FnMut(ComponentInfo, *mut u8),
    ) -> (Slot, Option<Entity>) {
        let entity = self.entity(slot).expect("Invalid entity");
        let dst_slot = dst.allocate(entity);
        let last = self.len - 1;

        for storage in self.storage.values_mut() {
            let p = storage.at_mut(slot);
            match dst.put_dyn(dst_slot, &storage.component, p) {
                Err(p) => (on_drop)(storage.component, p),
                _ => {}
            };

            // Move back in to fill the gap
            if slot != last {
                let p_last = storage.at_mut(last);
                std::ptr::copy_nonoverlapping(p_last, p, storage.component.layout.size());
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

    /// Move all components of an entity out of an archetype
    pub unsafe fn take(
        &mut self,
        slot: Slot,
        mut on_take: impl FnMut(ComponentInfo, *mut u8),
    ) -> Option<Entity> {
        let _ = self.entity(slot).expect("Invalid entity");
        let last = self.len - 1;

        for storage in self.storage.values_mut() {
            let src = storage.at_mut(slot);
            (on_take)(storage.component, src);

            // Move back in to fill the gap
            if slot != last {
                let dst = storage.at_mut(last);
                std::ptr::copy_nonoverlapping(dst, src, storage.component.layout.size());
            }
        }

        self.len -= 1;

        if slot != last {
            self.entities[slot] = self.entities[last];
            Some(std::mem::take(&mut self.entities[last]).expect("Invalid entity at last pos"))
        } else {
            None
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
            for storage in self.storage.values_mut() {
                let new_layout = Layout::from_size_align(
                    storage.component.layout.size() * new_cap,
                    storage.component.layout.align(),
                )
                .unwrap();
                let new_data = alloc(new_layout);

                let data = storage.data.get_mut();
                if old_cap > 0 {
                    // Copy over the previous contiguous data
                    std::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        new_data,
                        storage.component.layout.size() * self.len,
                    );

                    dealloc(
                        data.as_ptr(),
                        Layout::from_size_align(
                            storage.component.layout.size() * old_cap,
                            storage.component.layout.align(),
                        )
                        .unwrap(),
                    );
                }

                *storage.data.get_mut() = NonNull::new(new_data).unwrap();
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

    /// Drops all components while keeping the storage intact
    pub fn clear(&mut self) {
        for storage in self.storage.values_mut() {
            for slot in 0..self.len {
                unsafe {
                    let value = storage
                        .data
                        .get_mut()
                        .as_ptr()
                        .add(slot * storage.component.layout.size());
                    (storage.component.drop)(value);
                }
            }
        }

        self.len = 0;
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
    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.storage.values().map(|v| &v.component)
    }
}

impl Drop for Archetype {
    fn drop(&mut self) {
        self.clear();
        if self.cap > 0 {
            for storage in self.storage.values_mut() {
                // Handle ZST
                if storage.component.layout.size() > 0 {
                    unsafe {
                        dealloc(
                            storage.data.get_mut().as_ptr(),
                            Layout::from_size_align(
                                storage.component.layout.size() * self.cap,
                                storage.component.layout.align(),
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
    data: AtomicRef<'a, [T]>,
    id: ComponentId,
}

impl<'a, T> StorageBorrow<'a, T> {
    /// # Panics
    /// If the entity does not exist in the storage
    pub fn at(&self, slot: Slot) -> &T {
        &self.data[slot]
    }
}

pub struct StorageBorrowMut<'a, T> {
    data: AtomicRefMut<'a, [T]>,
    id: ComponentId,
}

impl<'a, T> StorageBorrowMut<'a, T> {
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

#[derive(Debug)]
/// Holds components for a single type
pub(crate) struct Storage {
    data: AtomicRefCell<NonNull<u8>>,
    component: ComponentInfo,
}

impl Storage {
    /// # Panics
    /// If the entity does not exist in the storage
    pub(crate) unsafe fn at(&mut self, slot: Slot) -> *mut u8 {
        self.data
            .get_mut()
            .as_ptr()
            .add(self.component.layout.size() * slot)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
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

    use crate::{component, entity::EntityKind, ComponentBuffer};

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

        let id = Entity::from_parts(NonZeroU32::new(6).unwrap(), 2, EntityKind::empty());
        let id_2 = Entity::from_parts(NonZeroU32::new(5).unwrap(), 2, EntityKind::empty());

        let slot = arch.insert(id, &mut buffer);
        eprintln!("Slot: {slot}");

        // Reuse buffer and insert again
        buffer.insert(a(), 9);
        buffer.insert(b(), "Bar".to_string());
        buffer.insert(c(), shared.clone());

        let slot_2 = arch.insert(id_2, &mut buffer);

        assert_eq!(slot, 0);
        assert_eq!(arch.get(slot, a()).as_deref(), Some(&7));
        assert_eq!(arch.get(slot, b()).as_deref(), Some(&"Foo".to_string()));
        assert_eq!(arch.get(slot_2, b()).as_deref(), Some(&"Bar".to_string()));

        arch.get_mut(slot, b())
            .unwrap()
            .push_str(&"Bar".to_string());

        assert_eq!(arch.get(slot, b()).as_deref(), Some(&"FooBar".to_string()));
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
        std::slice::from_raw_parts_mut(self.data.get_mut().as_ptr().cast(), len)
    }

    /// # Safety
    /// Assumes the type `T` is compatible with the stored type.
    /// `len` is the length of the allocated slice in T
    unsafe fn as_slice<T>(&mut self, len: usize) -> &mut [T] {
        std::slice::from_raw_parts_mut(self.data.get_mut().as_ptr().cast(), len)
    }

    /// Returns the `index`th element of type represented by info.
    unsafe fn elem_raw(&mut self, index: usize, info: &ComponentInfo) -> *mut u8 {
        self.data.get_mut().as_ptr().add(index * info.layout.size())
    }

    unsafe fn at_mut(&mut self, slot: Slot) -> *mut u8 {
        self.data
            .get_mut()
            .as_ptr()
            .add(self.component.layout.size() * slot)
    }
}

const HIGH_BIT: u32 = !(u32::MAX >> 1);
