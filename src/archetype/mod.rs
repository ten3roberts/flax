use std::{
    alloc::{alloc, dealloc, Layout},
    collections::BTreeMap,
    mem,
    ptr::NonNull,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use itertools::Itertools;

use crate::{
    wildcard, Component, ComponentBuffer, ComponentId, ComponentValue, Entity, EntityKind,
};

pub type ArchetypeId = Entity;
pub type Slot = usize;

mod changes;
mod slice;
mod visit;

pub use changes::*;
pub use slice::*;
pub use visit::*;

#[derive(Debug)]
pub struct Archetype {
    storage: BTreeMap<Entity, Storage>,
    changes: BTreeMap<ComponentId, AtomicRefCell<Changes>>,
    /// Slot to entity id
    entities: Vec<Entity>,
    // Number of slots
    cap: usize,

    // ComponentId => ArchetypeId
    // If the key is an existing component, it means it is a backwards edge
    edges: BTreeMap<Entity, ArchetypeId>,
}

/// Since all components are Send + Sync, the archetype is as well
unsafe impl Send for Archetype {}
unsafe impl Sync for Archetype {}

impl Archetype {
    pub fn empty() -> Self {
        Self {
            storage: BTreeMap::new(),
            changes: BTreeMap::new(),
            cap: 0,
            edges: BTreeMap::new(),
            entities: Vec::new(),
        }
    }

    pub fn relations(&self) -> impl Iterator<Item = ComponentId> + '_ {
        self.storage
            .keys()
            .filter(|v| v.kind().contains(EntityKind::RELATION))
            .copied()
    }

    /// Returns the components with the specified relation type.
    pub fn relations_like(&self, relation: Entity) -> impl Iterator<Item = Entity> + '_ {
        let relation = relation.low();

        self.relations().filter(move |k| k.low() == relation)
    }

    /// Returns all relations matching the relation type if the object is a
    /// wildcard, otherwise, returns an exact match
    pub fn matches_relation(&self, relation: Entity) -> impl Iterator<Item = Entity> + '_ {
        let (rel, obj) = relation.split_pair();
        let is_wild = obj == wildcard().low();
        self.relations().filter(move |&v| {
            let (low, high) = v.split_pair();
            is_wild && low == rel || !is_wild && v == relation
        })
    }

    /// Create a new archetype.
    /// Assumes `components` are sorted by id.
    pub(crate) fn new(components: impl IntoIterator<Item = ComponentInfo>) -> Self {
        let (storage, changes) = components
            .into_iter()
            .map(|component| {
                (
                    (
                        component.id,
                        Storage {
                            data: AtomicRefCell::new(NonNull::dangling()),
                            info: component,
                        },
                    ),
                    (component.id, AtomicRefCell::new(Changes::new(component))),
                )
            })
            .unzip();

        Self {
            cap: 0,
            storage,
            changes,
            edges: BTreeMap::new(),
            entities: Vec::new(),
        }
    }

    pub fn slots(&self) -> Slice {
        Slice::new(0, self.len())
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
        let len = self.len();
        let storage = self.storage.get(&component.id())?;

        // Type is guaranteed by `map`
        let data = storage
            .data
            .try_borrow_mut()
            .map_err(|_| format!("Component {} is already borrowed", storage.info.name()))
            .unwrap();

        let data = AtomicRefMut::map(data, |v| unsafe {
            std::slice::from_raw_parts_mut(v.as_ptr().cast::<T>(), len)
        });

        Some(StorageBorrowMut { data })
    }

    pub fn init_changes(&mut self, info: ComponentInfo) -> &mut Changes {
        self.changes
            .entry(info.id())
            .or_insert_with(|| {
                tracing::debug!("Initialized changes for {}", info.name);
                AtomicRefCell::new(Changes::new(info))
            })
            .get_mut()
    }

    // pub fn remove_slot_changes(&mut self, slot: Slot) {
    //     for (&component, changes) in self.changes.iter_mut() {
    //         eprintln!("Removed changes in component {component:?}");
    //         changes.get_mut().remove(slot);
    //     }
    // }

    /// Removes a slot and swaps in the last slot
    unsafe fn remove_slot(
        &mut self,
        slot: Slot,
        mut sink: impl FnMut(ComponentInfo, Vec<Change>),
    ) -> Option<(Entity, Slot)> {
        let last = self.len() - 1;
        if slot != last {
            for (_, changes) in self.changes.iter_mut() {
                let changes = changes.get_mut();
                sink(changes.info(), changes.swap_out(slot, last))
            }

            self.entities[slot] = self.entities[last];
            Some((self.entities.pop().unwrap(), slot))
        } else {
            for (_, changes) in self.changes.iter_mut() {
                let changes = changes.get_mut();
                sink(changes.info(), changes.remove(slot));
            }
            self.entities.pop().expect("Non empty");

            None
        }
    }

    fn migrate_changes(&mut self, other: &mut Self, src_slot: Slot, dst_slot: Slot) {
        for (_, changes) in self.changes.iter_mut() {
            let changes = changes.get_mut();
            let other = other.init_changes(changes.info());
            changes.migrate_to(other, src_slot, dst_slot)
        }
    }

    /// Borrow the change list
    pub fn changes(&self, component: ComponentId) -> Option<AtomicRef<Changes>> {
        let changes = self.changes.get(&component)?.borrow();
        Some(changes)
    }

    /// Borrow the change list mutably
    pub fn changes_mut(&self, component: ComponentId) -> Option<AtomicRefMut<Changes>> {
        let changes = self.changes.get(&component)?.borrow_mut();
        Some(changes)
    }

    #[inline]
    pub fn storage<T: ComponentValue>(&self, component: Component<T>) -> Option<StorageBorrow<T>> {
        let storage = self.storage.get(&component.id())?;
        // Type is guaranteed by `map`
        let data = storage
            .data
            .try_borrow()
            .map_err(|_| {
                format!(
                    "Component {} is already borrowed mutably",
                    storage.info.name()
                )
            })
            .unwrap();

        let data = AtomicRef::map(data, |v| unsafe {
            std::slice::from_raw_parts_mut(v.as_ptr().cast::<T>(), self.len())
        });

        Some(StorageBorrow { data })
    }

    pub(crate) fn storage_from_id<T: ComponentValue>(
        &self,
        id: ComponentId,
    ) -> Option<StorageBorrow<T>> {
        let storage = self.storage.get(&id)?;
        // Type is guaranteed by `map`
        let data = storage.data.borrow();
        let data = AtomicRef::map(data, |v| unsafe {
            std::slice::from_raw_parts_mut(v.as_ptr().cast::<T>(), self.len())
        });

        Some(StorageBorrow { data })
    }

    pub(crate) fn storage_raw(&mut self, component: ComponentId) -> Option<&mut Storage> {
        self.storage.get_mut(&component)
    }

    /// Borrow a storage dynamically
    pub fn storage_dyn(&self, component: ComponentId) -> Option<StorageBorrowDyn> {
        let storage = self.storage.get(&component)?;

        let data = storage.data.borrow();

        Some(StorageBorrowDyn {
            data,
            info: storage.info(),
            len: self.len(),
        })
    }

    pub fn get_unique<T: ComponentValue>(
        &mut self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<&mut T> {
        let len = self.len();
        let storage = self.storage.get_mut(&component.id())?;

        if slot < len {
            let v = storage.data.get_mut();
            unsafe { Some(&mut *(v.as_ptr().cast::<T>().add(slot))) }
        } else {
            None
        }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get_mut<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRefMut<T>> {
        let storage = self.storage.get(&component.id())?;

        if slot < self.len() {
            Some(AtomicRefMut::map(storage.data.borrow_mut(), |v| unsafe {
                &mut *(v.as_ptr().cast::<T>().add(slot))
            }))
        } else {
            None
        }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get_dyn(&mut self, slot: Slot, component: ComponentId) -> Option<*mut u8> {
        let len = self.len();
        let storage = self.storage.get_mut(&component)?;
        let info = &storage.info;

        if slot < len {
            let data = storage.data.get_mut();
            unsafe { Some(data.as_ptr().add(slot * info.size())) }
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
        let storage = self.storage.get(&component.id())?;

        if slot < self.len() {
            Some(AtomicRef::map(storage.data.borrow(), |v| unsafe {
                &*(v.as_ptr().cast::<T>().add(slot))
            }))
        } else {
            None
        }
    }

    /// Insert a new entity into the archetype.
    /// The components must match exactly.
    ///
    /// Returns the index of the entity
    /// Entity must not exist in archetype
    pub fn insert(&mut self, id: Entity, components: &mut ComponentBuffer) -> Slot {
        let slot = self.allocate(id);
        unsafe {
            for (component, src) in components.take_all() {
                let storage = self.storage.get_mut(&component.id).unwrap();
                std::ptr::copy_nonoverlapping(
                    src,
                    storage.data.get_mut().as_ptr().add(component.size() * slot),
                    component.size(),
                );
            }
        }

        slot
    }

    /// Allocated space for a new slot.
    /// The slot will always be greater than any previous call.
    /// # Safety
    /// All components of slot are uninitialized. `pub_dyn` needs to be called for
    /// all components in archetype.
    pub(crate) fn allocate(&mut self, id: Entity) -> Slot {
        self.reserve(1);

        let slot = self.len();

        self.entities.push(id);

        slot
    }

    /// Allocates consecutive slots.
    /// Returns the new slots
    ///
    /// # Safety
    /// All components of the new slots are left uninitialized.
    fn allocate_n(&mut self, ids: &[Entity]) -> Slice {
        self.reserve(ids.len());

        let last = self.len();

        self.entities.extend(ids);

        Slice::new(last, self.len())
    }

    /// Put a type erased component info a slot.
    /// `src` shall be considered moved.
    /// `component` must match the type of data.
    /// # Safety
    /// Must be called only **ONCE**. Returns Err(src) if move was unsuccessful
    /// The component must be Send + Sync
    pub unsafe fn put_dyn(
        &mut self,
        slot: Slot,
        component: &ComponentInfo,
        src: *mut u8,
    ) -> Result<(), *mut u8> {
        let storage = self.storage.get_mut(&component.id).ok_or(src)?;

        assert_eq!(component.id(), storage.info.id());
        let dst = storage.at_mut(slot);
        std::ptr::copy_nonoverlapping(src, dst, component.size());

        Ok(())
    }

    /// Move all components in `slot` to archetype of `dst`. The components not
    /// in self will be left uninitialized.
    /// # Safety
    /// `dst.put_dyn` must be called immediately after for each missing
    /// component.
    ///
    /// Returns the slot in dst and entity which was moved into current `slot`, if any.
    pub unsafe fn move_to(
        &mut self,
        dst: &mut Self,
        slot: Slot,
        mut on_drop: impl FnMut(ComponentInfo, *mut u8),
    ) -> (Slot, Option<(Entity, Slot)>) {
        let entity = self.entity(slot).expect("Invalid entity");
        let dst_slot = dst.allocate(entity);
        let last = self.len() - 1;

        for storage in self.storage.values_mut() {
            let p = storage.at_mut(slot);
            if let Err(p) = dst.put_dyn(dst_slot, &storage.info, p) {
                (on_drop)(storage.info, p)
            };

            // Move back in to fill the gap
            if slot != last {
                let p_last = storage.at_mut(last);
                std::ptr::copy_nonoverlapping(p_last, p, storage.info.size());
            }
        }

        let swapped = self.remove_slot(slot, |info, changes| {
            let other = dst.init_changes(info);
            for mut change in changes {
                assert_eq!(change.slice, Slice::single(slot));
                change.slice = Slice::single(dst_slot);
                other.set(change);
            }
        });

        (dst_slot, swapped)
    }

    /// Move all components of an entity out of an archetype
    ///
    /// Returns the entity which filled the now empty slot
    ///
    /// # Safety
    /// The callee is responsible to store or drop the returned components using
    /// the `on_take` function.
    pub unsafe fn take(
        &mut self,
        slot: Slot,
        mut on_take: impl FnMut(ComponentInfo, *mut u8),
    ) -> Option<(Entity, Slot)> {
        let _ = self.entity(slot).expect("Invalid entity");
        let last = self.len() - 1;

        for storage in self.storage.values_mut() {
            let src = storage.at_mut(slot);
            (on_take)(storage.info, src);

            // Move back in to fill the gap
            if slot != last {
                let dst = storage.at_mut(last);
                std::ptr::copy_nonoverlapping(dst, src, storage.info.size());
            }
        }

        self.remove_slot(slot, |_, _| {})
    }

    /// Move all entities from one archetype to another.
    ///
    /// Leaves `self` empty.
    /// Returns the new location of all entities
    pub fn move_all(&mut self, dst: &mut Self) -> Vec<(Entity, Slot)> {
        let len = self.len();
        // Storage is dangling
        if len == 0 {
            return Vec::new();
        }

        let entities = mem::take(&mut self.entities);
        eprintln!("Entities: {entities:?}");

        let dst_slots = dst.allocate_n(&entities);
        eprintln!("Allocated {dst_slots:?} for move all");

        // Migrate all changes before doing anything
        for (src_slot, dst_slot) in self.slots().iter().zip(dst_slots) {
            self.migrate_changes(dst, src_slot, dst_slot)
        }

        for (id, storage) in &mut self.storage {
            if let Some(dst_storage) = dst.storage_raw(*id) {
                // Copy this storage to the end of dst
                if storage.info.size() > 0 {
                    unsafe {
                        let src = storage.data.get_mut().as_ptr();
                        let dst = dst_storage.at_mut(dst_slots.start);
                        dbg!(dst, dst_storage.data.get_mut());
                        std::ptr::copy_nonoverlapping(src, dst, len * storage.info().size())
                    }
                }
            } else {
                // Drop this whole column
                // eprintln!("Dropping all data in {:?}", storage.info());

                for slot in 0..len {
                    if storage.info.size() > 0 {
                        unsafe {
                            let value = storage.at_mut(slot);
                            (storage.info.drop)(value);
                        }
                    }
                }
            }
        }

        assert_eq!(self.len(), 0);

        entities.iter().cloned().zip(dst_slots.iter()).collect_vec()
    }

    /// Reserves space for atleast `additional` entities.
    /// Does nothing if the remaining capacity < additional.
    /// len remains unchanged, as does the internal order
    pub fn reserve(&mut self, additional: usize) {
        let len = self.len();
        let old_cap = self.cap;
        let new_cap = (len + additional).next_power_of_two();

        if new_cap <= old_cap {
            return;
        }

        assert_ne!(additional, 0, "Length is: {len}");
        unsafe {
            for storage in self.storage.values_mut() {
                if storage.info.size() == 0 {
                    continue;
                }

                let new_layout = Layout::from_size_align(
                    storage.info.size() * new_cap,
                    storage.info.layout.align(),
                )
                .unwrap();
                let new_data = alloc(new_layout);

                let data = storage.data.get_mut().as_ptr();
                if old_cap > 0 {
                    // Copy over the previous contiguous data
                    std::ptr::copy_nonoverlapping(data, new_data, storage.info.size() * len);

                    dealloc(
                        data,
                        Layout::from_size_align(
                            storage.info.size() * old_cap,
                            storage.info.layout.align(),
                        )
                        .unwrap(),
                    );
                }

                *storage.data.get_mut() = NonNull::new(new_data).unwrap();
            }
        }

        // Copy over entity ids
        // let mut new_entities = vec![None; new_cap].into_boxed_slice();
        // new_entities[0..self.len].copy_from_slice(&self.entities[0..self.len]);
        // self.entities = new_entities;
        self.cap = new_cap;
    }

    pub fn entity(&self, slot: Slot) -> Option<Entity> {
        self.entities.get(slot).copied()
    }

    /// Drops all components while keeping the storage intact
    pub fn clear(&mut self) {
        let len = self.len();
        for storage in self.storage.values_mut() {
            for slot in 0..len {
                unsafe {
                    let value = storage.at_mut(slot);
                    (storage.info.drop)(value);
                }
            }
        }

        self.entities.clear();
    }

    #[must_use]
    // Number of entities in the archetype
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Get the archetype's cap.
    #[must_use]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Get a reference to the archetype's components.
    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.storage.values().map(|v| &v.info)
    }

    pub fn component_names(&self) -> impl Iterator<Item = &str> {
        self.storage.values().map(|v| v.info.name())
    }

    pub fn storages(&self) -> impl Iterator<Item = StorageBorrowDyn> {
        self.components().map(|v| self.storage_dyn(v.id()).unwrap())
    }

    /// Access the entities in the archetype for each slot. Entity is None if
    /// the slot is not occupied, only for the last slots.
    pub fn entities(&self) -> &[Entity] {
        self.entities.as_ref()
    }
}

impl Drop for Archetype {
    fn drop(&mut self) {
        self.clear();

        if self.cap > 0 {
            for storage in self.storage.values_mut() {
                // Handle ZST
                if storage.info.size() > 0 {
                    unsafe {
                        dealloc(
                            storage.data.get_mut().as_ptr(),
                            Layout::from_size_align(
                                storage.info.size() * self.cap,
                                storage.info.layout.align(),
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

/// Type erased atomic borrow of a component
pub struct StorageBorrowDyn<'a> {
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

    pub fn info(&self) -> ComponentInfo {
        self.info
    }
}

#[derive(Debug)]
/// Holds components for a single type
pub(crate) struct Storage {
    data: AtomicRefCell<NonNull<u8>>,
    info: ComponentInfo,
}

impl Storage {
    pub(crate) unsafe fn at_mut(&mut self, slot: Slot) -> *mut u8 {
        self.data.get_mut().as_ptr().add(self.info.size() * slot)
    }

    pub(crate) fn info(&self) -> ComponentInfo {
        self.info
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct ComponentInfo {
    pub(crate) layout: Layout,
    pub(crate) id: ComponentId,
    pub(crate) name: &'static str,
    pub(crate) drop: unsafe fn(*mut u8),
    meta: fn(Self) -> ComponentBuffer,
}

// impl std::fmt::Debug for ComponentInfo {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("ComponentInfo")
//             .field("id", &self.id)
//             .field("name", &self.name)
//             .finish()
//     }
// }

impl<T: ComponentValue> From<Component<T>> for ComponentInfo {
    fn from(v: Component<T>) -> Self {
        v.info()
    }
}

impl PartialOrd for ComponentInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl Ord for ComponentInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl ComponentInfo {
    pub fn of<T: ComponentValue>(component: Component<T>) -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }
        Self {
            drop: drop_ptr::<T>,
            layout: Layout::new::<T>(),
            id: component.id(),
            name: component.name(),
            meta: component.meta(),
        }
    }

    pub(crate) fn size(&self) -> usize {
        self.layout.size()
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn id(&self) -> Entity {
        self.id
    }

    pub fn meta(&self) -> fn(ComponentInfo) -> ComponentBuffer {
        self.meta
    }

    pub(crate) fn get_meta(&self) -> ComponentBuffer {
        (self.meta)(*self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{component, ComponentBuffer, EntityKind};

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
        buffer.set(a(), 7);
        buffer.set(b(), "Foo".to_string());
        buffer.set(c(), shared.clone());

        let id = Entity::from_parts(NonZeroU32::new(6).unwrap(), 2, EntityKind::empty());
        let id_2 = Entity::from_parts(NonZeroU32::new(5).unwrap(), 2, EntityKind::empty());

        let slot = arch.insert(id, &mut buffer);
        eprintln!("Slot: {slot}");

        // Reuse buffer and insert again
        buffer.set(a(), 9);
        buffer.set(b(), "Bar".to_string());
        buffer.set(c(), shared.clone());

        let slot_2 = arch.insert(id_2, &mut buffer);

        assert_eq!(slot, 0);
        assert_eq!(arch.get(slot, a()).as_deref(), Some(&7));
        assert_eq!(arch.get(slot, b()).as_deref(), Some(&"Foo".to_string()));
        assert_eq!(arch.get(slot_2, b()).as_deref(), Some(&"Bar".to_string()));

        arch.get_mut(slot, b()).unwrap().push_str("Bar");

        assert_eq!(arch.get(slot, b()).as_deref(), Some(&"FooBar".to_string()));
        assert_eq!(arch.entity(slot), Some(id));
        assert_eq!(arch.entity(slot_2), Some(id_2));

        drop(arch);

        assert_eq!(Arc::strong_count(&shared), 1);
    }
}
