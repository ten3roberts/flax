use alloc::{collections::BTreeMap, format, vec::Vec};
use core::{alloc::Layout, any::TypeId, fmt::Debug, mem};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use itertools::Itertools;

use crate::{
    buffer::ComponentBuffer, entity, Component, ComponentId, ComponentValue, Entity, Verbatim,
};

/// Unique archetype id
pub type ArchetypeId = Entity;
/// Represents a slot in the archetype
pub type Slot = usize;

mod batch;
mod changes;
mod slice;
mod storage;

pub use batch::*;
pub use changes::*;
pub use slice::*;
pub(crate) use storage::*;

#[derive(Debug, Clone)]
/// Holds information of a single component storage buffer
pub struct StorageInfo {
    cap: usize,
    len: usize,
}

impl StorageInfo {
    /// Returns the storage capacity
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Returns the length of the buffer
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    /// Returns true if the storage is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

const SHORT_DEBUG_LEN: usize = 8;
#[derive(Clone)]
/// Shows only a handful of entries to avoid cluttering the terminal with gigantic vecs
struct ShortDebugVec<T>(Vec<T>);

impl<T> Default for ShortDebugVec<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T: Debug> Debug for ShortDebugVec<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut s = f.debug_list();
        s.entries(self.0.iter().take(SHORT_DEBUG_LEN));

        if self.0.len() > SHORT_DEBUG_LEN {
            s.entry(&Verbatim(&format!(
                "+{} more",
                self.0.len() - SHORT_DEBUG_LEN
            )));
        }

        s.finish()
    }
}

/// Human friendly archetype inspection
#[derive(Default, Debug, Clone)]
pub struct ArchetypeInfo {
    storage: Vec<StorageInfo>,
    components: Vec<ComponentInfo>,
    entities: ShortDebugVec<Entity>,
}

impl ArchetypeInfo {
    /// Returns information about archetype storages
    pub fn storage(&self) -> &[StorageInfo] {
        self.storage.as_ref()
    }

    /// Returns the components in the archetype
    pub fn components(&self) -> &[ComponentInfo] {
        self.components.as_ref()
    }

    /// Returns the entities in the archetype
    pub fn entities(&self) -> &[Entity] {
        &self.entities.0
    }
}

#[derive(Debug)]
#[doc(hidden)]
/// A collection of entities with the same components.
/// Stored as columns of contiguous component data.
pub struct Archetype {
    storage: BTreeMap<Entity, Storage>,
    changes: BTreeMap<ComponentId, AtomicRefCell<Changes>>,
    /// Slot to entity id
    pub(crate) entities: Vec<Entity>,

    // ComponentId => ArchetypeId
    pub(crate) outgoing: BTreeMap<ComponentId, (bool, ArchetypeId)>,
    pub(crate) incoming: BTreeMap<ComponentId, ArchetypeId>,
}

/// Since all components are Send + Sync, the archetype is as well
unsafe impl Send for Archetype {}
unsafe impl Sync for Archetype {}

impl Archetype {
    pub(crate) fn empty() -> Self {
        Self {
            storage: BTreeMap::new(),
            changes: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            entities: Vec::new(),
        }
    }

    /// Returns all the relation components in the archetype
    pub fn relations(&self) -> impl Iterator<Item = ComponentId> + '_ {
        self.storage.keys().filter(|v| v.is_relation()).copied()
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
        let is_wild = obj == crate::entity::wildcard().low();
        self.relations().filter(move |&v| {
            let (low, _) = v.split_pair();
            is_wild && low == rel || !is_wild && v == relation
        })
    }

    /// Create a new archetype.
    /// Assumes `components` are sorted by id.
    pub(crate) fn new(components: impl IntoIterator<Item = ComponentInfo>) -> Self {
        let (storage, changes) = components
            .into_iter()
            .map(|info| {
                let id = info.id();
                if !id.kind().contains(entity::EntityKind::COMPONENT) {
                    panic!("Attempt to insert non component entity");
                }

                (
                    (id, Storage::new(info)),
                    (id, AtomicRefCell::new(Changes::new(info))),
                )
            })
            .unzip();

        Self {
            storage,
            changes,
            incoming: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            entities: Vec::new(),
        }
    }

    /// Returns all the slots in the archetype
    pub fn slots(&self) -> Slice {
        Slice::new(0, self.len())
    }

    /// Returns true if the archtype has `component`
    pub fn has(&self, component: ComponentId) -> bool {
        self.storage.get(&component).is_some()
    }

    pub(crate) fn outgoing(&self, component: ComponentId) -> Option<(bool, ArchetypeId)> {
        self.outgoing.get(&component).copied()
    }

    pub(crate) fn incoming(&self, component: ComponentId) -> Option<ArchetypeId> {
        self.incoming.get(&component).copied()
    }

    pub(crate) fn add_incoming(&mut self, dst_id: ArchetypeId, component: ComponentId) {
        self.incoming.insert(component, dst_id);
    }

    pub(crate) fn add_outgoing(
        &mut self,
        dst_id: ArchetypeId,
        strong_link: bool,
        component: ComponentId,
    ) {
        let link = self
            .outgoing
            .entry(component)
            .or_insert((strong_link, dst_id));

        link.0 = link.0 || strong_link;
    }

    pub(crate) fn init_changes(&mut self, info: ComponentInfo) -> &mut Changes {
        self.changes
            .entry(info.id())
            .or_insert_with(|| AtomicRefCell::new(Changes::new(info)))
            .get_mut()
    }

    /// Access a component storage mutably.
    /// # Panics
    /// If the storage is already borrowed
    pub fn borrow_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> Option<AtomicRefMut<[T]>> {
        Some(unsafe { self.storage.get(&component.id())?.borrow_mut() })
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
        mut dst: Option<(&mut Self, Slot)>,
    ) -> Option<(Entity, Slot)> {
        let last = self.len() - 1;
        let len = self.len();
        if slot != last {
            for (_, changes) in self.changes.iter_mut() {
                let changes = changes.get_mut();
                let info = changes.info();
                if let Some((ref mut dst, dst_slot)) = dst {
                    let dst = dst.init_changes(info);

                    changes.swap_remove(slot, last, |kind, mut v| {
                        v.slice = Slice::single(dst_slot);
                        dst.set(kind, v);
                    });
                } else {
                    changes.swap_remove(slot, last, |_, _| {});
                }

                changes.get(ChangeKind::Inserted).iter().for_each(|v| {
                    assert!(v.slice.end <= len, "Changes have more slots than archetype")
                });
            }

            self.entities[slot] = self.entities[last];
            Some((self.entities.pop().unwrap(), slot))
        } else {
            for (_, changes) in self.changes.iter_mut() {
                let changes = changes.get_mut();
                if let Some((ref mut dst, dst_slot)) = dst {
                    changes.migrate_to(dst.init_changes(changes.info()), slot, dst_slot)
                } else {
                    changes.remove(slot);
                }
            }
            self.entities.pop().expect("Non empty");

            None
        }
    }

    /// Returns human friendly debug info
    pub fn info(&self) -> ArchetypeInfo {
        let (components, storage) = self
            .storage
            .values()
            .map(|v| {
                (
                    *v.info(),
                    StorageInfo {
                        cap: v.capacity(),
                        len: v.len(),
                    },
                )
            })
            .unzip();

        ArchetypeInfo {
            components,
            storage,
            entities: ShortDebugVec(self.entities.clone()),
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
    pub(crate) fn changes(&self, component: ComponentId) -> Option<AtomicRef<Changes>> {
        let changes = self.changes.get(&component)?.borrow();
        Some(changes)
    }

    /// Borrow the change list mutably
    pub(crate) fn changes_mut(&self, component: ComponentId) -> Option<AtomicRefMut<Changes>> {
        let changes = self.changes.get(&component)?.borrow_mut();
        Some(changes)
    }

    pub(crate) fn borrow<T: ComponentValue, I: Into<ComponentId>>(
        &self,
        component: I,
    ) -> Option<AtomicRef<[T]>> {
        Some(unsafe { self.storage.get(&component.into())?.borrow() })
    }

    /// Borrow a storage dynamically
    ///
    /// # Panics
    /// If the storage is already borrowed mutably
    pub(crate) fn borrow_dyn(&self, component: ComponentId) -> Option<StorageBorrowDyn> {
        Some(unsafe { self.storage.get(&component)?.borrow_dyn() })
    }

    /// Returns the value of a component from a unique access
    pub fn get_unique<T: ComponentValue>(
        &mut self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<&mut T> {
        let storage = self.storage.get_mut(&component.id())?;

        unsafe {
            let ptr = storage.at_mut(slot)?;
            Some(ptr.cast::<T>().as_mut().unwrap())
        }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get_mut<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRefMut<T>> {
        let storage = self.storage.get(&component.id())?;

        AtomicRefMut::filter_map(unsafe { storage.borrow_mut() }, |v| v.get_mut(slot))
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get_dyn(&mut self, slot: Slot, component: ComponentId) -> Option<*mut u8> {
        let storage = self.storage.get_mut(&component)?;

        unsafe { storage.at_mut(slot) }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        let storage = self.storage.get(&component.id())?;

        AtomicRef::filter_map(unsafe { storage.borrow() }, |v| v.get(slot))
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
                storage.extend(src, 1);
            }
        }

        slot
    }

    /// Allocated space for a new slot.
    /// The slot will always be greater than any previous call.
    /// # Safety
    /// All components of slot are uninitialized. Must be followed by `push`
    /// all components in archetype.
    pub(crate) fn allocate(&mut self, id: Entity) -> Slot {
        self.reserve(1);

        #[cfg(debug_assertions)]
        {
            if self.entities.iter().any(|&v| v == id) {
                panic!("Entity already in archetype");
            }
        }
        let slot = self.len();

        self.entities.push(id);

        slot
    }

    /// Allocates consecutive slots.
    /// Returns the new slots
    ///
    /// # Safety
    /// All components of the new slots are left uninitialized.
    /// Must be followed by `extend`
    pub(crate) fn allocate_n(&mut self, ids: &[Entity]) -> Slice {
        self.reserve(ids.len());

        let last = self.len();

        self.entities.extend(ids);

        Slice::new(last, self.len())
    }

    /// Put a type erased component into the new slot
    /// `src` shall be considered moved.
    /// `component` must match the type of data.
    /// # Safety
    /// Must be called only **ONCE**. Returns Err(src) if move was unsuccessful
    /// The component must be Send + Sync
    pub unsafe fn push(&mut self, component: ComponentId, src: *mut u8) -> Result<(), *mut u8> {
        let storage = self.storage.get_mut(&component).ok_or(src)?;
        storage.extend(src, 1);

        assert!(
            storage.len() <= self.entities.len(),
            "Attempt to insert more values than entities {} > {}",
            storage.len(),
            self.entities.len()
        );

        Ok(())
    }

    /// Moves the components in `storage` to the not yet initialized space in a
    /// new allocation.
    /// # Safety
    /// The length of the passed data must be equal to the slice and the slice
    /// must point to a currently uninitialized region in the archetype.
    pub(crate) unsafe fn extend(&mut self, src: &mut Storage) -> Option<usize> {
        let storage = self.storage.get_mut(&src.info().id())?;

        let additional = src.len();
        storage.append(src);
        assert!(storage.len() <= self.len());

        Some(additional)
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
        mut on_drop: impl FnMut(&ComponentInfo, *mut u8),
    ) -> (Slot, Option<(Entity, Slot)>) {
        let entity = self.entity(slot).expect("Invalid entity");
        let dst_slot = dst.allocate(entity);

        for (&id, storage) in &mut self.storage {
            let info = *storage.info();
            storage.swap_remove(slot, |p| {
                if let Err(p) = dst.push(id, p) {
                    (on_drop)(&info, p)
                }
            });
        }

        let swapped = self.remove_slot(slot, Some((dst, dst_slot)));

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
        mut on_take: impl FnMut(&ComponentInfo, *mut u8),
    ) -> Option<(Entity, Slot)> {
        let _ = self.entity(slot).expect("Invalid entity");

        for storage in self.storage.values_mut() {
            let info = *storage.info();
            storage.swap_remove(slot, |p| {
                (on_take)(&info, p);
            })
        }

        self.remove_slot(slot, None)
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

        let dst_slots = dst.allocate_n(&entities);

        // Migrate all changes before doing anything
        for (src_slot, dst_slot) in self.slots().iter().zip(dst_slots) {
            self.migrate_changes(dst, src_slot, dst_slot)
        }

        for storage in self.storage.values_mut() {
            // Copy this storage to the end of dst
            unsafe {
                let _ = dst.extend(storage);
            }
        }

        assert_eq!(self.len(), 0);

        entities.iter().cloned().zip(dst_slots.iter()).collect_vec()
    }

    /// Reserves space for atleast `additional` entities.
    /// Does nothing if the remaining capacity < additional.
    /// len remains unchanged, as does the internal order
    pub fn reserve(&mut self, additional: usize) {
        for storage in self.storage.values_mut() {
            storage.reserve(additional);
        }
    }

    /// Returns the entity at `slot`
    pub fn entity(&self, slot: Slot) -> Option<Entity> {
        self.entities.get(slot).copied()
    }

    /// Drops all components while keeping the storage intact
    pub fn clear(&mut self) {
        for storage in self.storage.values_mut() {
            storage.clear()
        }

        self.entities.clear();
    }

    #[must_use]
    /// Number of entities in the archetype
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[must_use]
    /// Returns true if the archetype contains no entities
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Get a reference to the archetype's components.
    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.storage.values().map(|v| v.info())
    }

    /// Returns the names of all components.
    /// Useful for debug purposes
    pub fn component_names(&self) -> impl Iterator<Item = &str> {
        self.storage.values().map(|v| v.info().name())
    }

    /// Returns a iterator which borrows each storage in the archetype
    pub(crate) fn borrow_all(&self) -> impl Iterator<Item = StorageBorrowDyn> {
        self.components().map(|v| self.borrow_dyn(v.id()).unwrap())
    }

    /// Access the entities in the archetype for each slot. Entity is None if
    /// the slot is not occupied, only for the last slots.
    pub fn entities(&self) -> &[Entity] {
        self.entities.as_ref()
    }

    pub(crate) fn storage(&self) -> &BTreeMap<Entity, Storage> {
        &self.storage
    }

    pub(crate) fn storage_mut(&mut self) -> &mut BTreeMap<Entity, Storage> {
        &mut self.storage
    }

    pub(crate) fn entities_mut(&mut self) -> &mut [Entity] {
        &mut self.entities
    }

    pub(crate) fn component(&self, component: Entity) -> Option<&ComponentInfo> {
        self.storage.get(&component).map(|v| v.info())
    }
}

impl Drop for Archetype {
    fn drop(&mut self) {
        self.clear();
    }
}

#[derive(Clone, PartialEq, Eq, Copy)]
/// Represents a type erased component along with its memory layout and drop fn.
/// Is essentially a vtable
pub struct ComponentInfo {
    pub(crate) layout: Layout,
    pub(crate) id: ComponentId,
    pub(crate) name: &'static str,
    pub(crate) drop: unsafe fn(*mut u8),
    pub(crate) type_id: TypeId,
    meta: fn(Self) -> ComponentBuffer,
}

impl Debug for ComponentInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComponentInfo")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

// impl core::fmt::Debug for ComponentInfo {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         f.debug_struct("ComponentInfo")
//             .field("id", &self.id)
//             .field("name", &self.name)
//             .finish()
//     }
// }

impl<T: ComponentValue> From<Component<T>> for ComponentInfo {
    fn from(v: Component<T>) -> Self {
        ComponentInfo::of(v)
    }
}

impl PartialOrd for ComponentInfo {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl Ord for ComponentInfo {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl ComponentInfo {
    /// Returns the component info of a types component
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
            type_id: TypeId::of::<T>(),
        }
    }

    pub(crate) fn is<T: ComponentValue>(&self) -> bool {
        self.type_id == TypeId::of::<T>()
    }

    pub(crate) fn size(&self) -> usize {
        self.layout.size()
    }

    /// Returns the component name
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the component id
    pub fn id(&self) -> Entity {
        self.id
    }

    /// Returns the component metadata fn
    pub fn meta(&self) -> fn(ComponentInfo) -> ComponentBuffer {
        self.meta
    }
}

#[cfg(test)]
mod tests {

    use crate::{component, entity::EntityKind};
    use alloc::string::{String, ToString};
    use alloc::sync::Arc;
    use alloc::vec;

    use super::*;
    use core::num::NonZeroU32;

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
