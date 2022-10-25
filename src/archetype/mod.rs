use alloc::{collections::BTreeMap, format, sync::Arc, vec::Vec};
use core::{
    alloc::Layout,
    any::{type_name, TypeId},
    fmt::Debug,
    mem,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use itertools::Itertools;

use crate::{
    buffer::ComponentBuffer, component, events::Subscriber, Component, ComponentKey,
    ComponentValue, Entity, Verbatim,
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

/// Stores a list of component values, changes, and subscribers
pub(crate) struct Cell {
    storage: AtomicRefCell<Storage>,
    changes: AtomicRefCell<Changes>,
    info: ComponentInfo,
    subscribers: Vec<Arc<dyn Subscriber>>,
}

impl Cell {
    /// Moves a slot in the cell to another cell and slot while migrating all changes.
    fn move_to(&mut self, slot: Slot, dst: &mut Self, dst_slot: Slot) {
        let storage = self.storage.get_mut();
        let changes = self.changes.get_mut();

        let last = storage.len() - 1;

        let dst_storage = dst.storage.get_mut();
        let dst_changes = dst.changes.get_mut();

        storage.swap_remove(slot, |p| unsafe {
            dst_storage.extend(p, 1);
        });

        // Replace this slot with the last slot and move everything to the dst archetype
        changes.swap_remove(slot, last, |kind, mut v| {
            v.slice = Slice::single(dst_slot);
            dst_changes.set(kind, v);
        });

        // Do not notify of removal, since the component is still intact, but in another archetype
    }

    /// Moves all slots to another cell
    fn move_all(&mut self, dst: &mut Self, dst_start: Slot) {
        let storage = self.storage.get_mut();
        let changes = self.changes.get_mut();

        let last = storage.len() - 1;

        let dst_storage = dst.storage.get_mut();
        let dst_changes = dst.changes.get_mut();

        assert_eq!(dst_storage.len(), dst_start);
        unsafe { dst_storage.append(storage) }

        changes.zip_map(dst_changes, |kind, a, b| {
            a.drain(..).for_each(|mut change| {
                change.slice.start += dst_start;
                change.slice.end += dst_start;

                b.set(change);
            })
        });
    }

    /// Move a slot out of the cell by swapping with the last
    fn take(&mut self, slot: Slot, mut on_move: impl FnMut(ComponentInfo, *mut u8)) {
        let storage = self.storage.get_mut();
        let changes = self.changes.get_mut();

        let last = storage.len() - 1;

        storage.swap_remove(slot, |p| on_move(self.info, p));
        changes.swap_remove(slot, last, |_, _| {});

        // Notify the subscribers that something was removed
        self.subscribers
            .iter()
            .for_each(|v| v.on_change(self.info.key(), ChangeKind::Removed))
    }

    /// Clears (and drops) all components and changes.
    fn clear(&mut self) {
        let storage = self.storage.get_mut();
        let changes = self.changes.get_mut();

        storage.clear();
        changes.clear();

        // Notify subscribers
        self.subscribers
            .iter()
            .for_each(|v| v.on_change(self.info.key(), ChangeKind::Removed));
    }

    /// Drain the values in the cell.
    pub(crate) fn drain(&mut self) -> Storage {
        let storage = mem::replace(self.storage.get_mut(), Storage::new(self.info));
        self.changes.get_mut().clear();

        // Notify subscribers
        self.subscribers
            .iter()
            .for_each(|v| v.on_change(self.info.key(), ChangeKind::Removed));
        storage
    }

    pub(crate) fn storage(&self) -> &AtomicRefCell<Storage> {
        &self.storage
    }
}

impl Drop for Cell {
    fn drop(&mut self) {
        if self.storage.get_mut().len() > 0 {
            // Notify subscribers
            self.subscribers
                .iter()
                .for_each(|v| v.on_change(self.info.key(), ChangeKind::Removed));
        }
    }
}

// #[derive(Debug)]
#[doc(hidden)]
/// A collection of entities with the same components.
/// Stored as columns of contiguous component data.
pub struct Archetype {
    cells: BTreeMap<ComponentKey, Cell>,
    /// Stores removals of components which transferred the entities to this archetype
    removals: BTreeMap<ComponentKey, ChangeList>,
    /// Slot to entity id
    pub(crate) entities: Vec<Entity>,

    // ComponentId => ArchetypeId
    pub(crate) outgoing: BTreeMap<ComponentKey, (bool, ArchetypeId)>,
    pub(crate) incoming: BTreeMap<ComponentKey, ArchetypeId>,

    pub(crate) subscribers: Vec<Arc<dyn Subscriber>>,
}

/// Since all components are Send + Sync, the archetype is as well
unsafe impl Send for Archetype {}
unsafe impl Sync for Archetype {}

impl Archetype {
    pub(crate) fn empty() -> Self {
        Self {
            cells: BTreeMap::new(),
            removals: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            entities: Vec::new(),
            subscribers: Vec::new(),
        }
    }

    /// Returns all the relation components in the archetype
    pub fn relations(&self) -> impl Iterator<Item = ComponentKey> + '_ {
        self.cells.keys().filter(|v| v.is_relation()).copied()
    }

    /// Returns the components with the specified relation type.
    pub fn relations_like(&self, relation: Entity) -> impl Iterator<Item = ComponentKey> + '_ {
        self.relations().filter(move |k| k.id == relation)
    }

    /// Create a new archetype.
    /// Assumes `components` are sorted by id.
    pub(crate) fn new<I>(components: I) -> Self
    where
        I: IntoIterator<Item = ComponentInfo>,
    {
        let cells = components
            .into_iter()
            .map(|info| {
                let key = info.key();

                (
                    key,
                    Cell {
                        info,
                        storage: AtomicRefCell::new(Storage::new(info)),
                        changes: AtomicRefCell::new(Changes::new(info)),
                        subscribers: Vec::new(),
                    },
                )
            })
            .collect();

        Self {
            cells,
            removals: BTreeMap::new(),
            incoming: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            entities: Vec::new(),
            subscribers: Vec::new(),
        }
    }

    /// Returns all the slots in the archetype
    pub fn slots(&self) -> Slice {
        Slice::new(0, self.len())
    }

    /// Returns true if the archtype has `component`
    pub fn has(&self, component: ComponentKey) -> bool {
        self.cells.get(&component).is_some()
    }

    pub(crate) fn outgoing(&self, component: ComponentKey) -> Option<(bool, ArchetypeId)> {
        self.outgoing.get(&component).copied()
    }

    pub(crate) fn incoming(&self, component: ComponentKey) -> Option<ArchetypeId> {
        self.incoming.get(&component).copied()
    }

    pub(crate) fn add_incoming(&mut self, dst_id: ArchetypeId, component: ComponentKey) {
        self.incoming.insert(component, dst_id);
    }

    pub(crate) fn add_outgoing(
        &mut self,
        dst_id: ArchetypeId,
        strong_link: bool,
        component: ComponentKey,
    ) {
        let link = self
            .outgoing
            .entry(component)
            .or_insert((strong_link, dst_id));

        link.0 = link.0 || strong_link;
    }

    fn push_removed(&mut self, key: ComponentKey, change: Change) {
        self.removals.entry(key).or_default().set(change);
    }

    pub(crate) fn borrow<T: ComponentValue>(
        &self,
        component: ComponentKey,
    ) -> Option<AtomicRef<[T]>> {
        let storage = self.cells.get(&component)?.storage.borrow();
        Some(AtomicRef::map(storage, |v| unsafe { v.borrow() }))
    }

    /// Access a component storage mutably.
    /// # Panics
    /// If the storage is already borrowed
    pub fn borrow_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
    ) -> Option<AtomicRefMut<[T]>> {
        let storage = self.cells.get(&component.key())?.storage.borrow_mut();
        Some(AtomicRefMut::map(storage, |v| unsafe { v.borrow_mut() }))
    }

    /// Removes a slot and swaps in the last slot
    #[inline(always)]
    unsafe fn remove_slot(&mut self, slot: Slot) -> Option<(Entity, Slot)> {
        let last = self.len() - 1;
        let len = self.len();
        if slot != last {
            self.entities[slot] = self.entities[last];
            Some((self.entities.pop().unwrap(), slot))
        } else {
            self.entities.pop().expect("Non empty");

            None
        }
    }

    /// Returns human friendly debug info
    pub fn info(&self) -> ArchetypeInfo {
        let (components, storage) = self
            .cells
            .values()
            .map(|v| {
                let s = v.storage.borrow();
                (
                    v.info,
                    StorageInfo {
                        cap: s.capacity(),
                        len: s.len(),
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

    /// Borrow the change list
    pub(crate) fn changes(&self, component: ComponentKey) -> Option<AtomicRef<Changes>> {
        let changes = self.cells.get(&component)?.changes.borrow();
        Some(changes)
    }

    pub(crate) fn removals(&self, component: ComponentKey) -> Option<&ChangeList> {
        self.removals.get(&component)
    }

    /// Borrow the change list mutably
    pub(crate) fn changes_mut(&self, component: ComponentKey) -> Option<AtomicRefMut<Changes>> {
        let changes = self.cells.get(&component)?.changes.borrow_mut();
        Some(changes)
    }

    /// Returns the value of a component from a unique access
    pub fn get_unique<T: ComponentValue>(
        &mut self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<&mut T> {
        let storage = self.cells.get_mut(&component.key())?.storage.get_mut();

        unsafe { storage.get_mut(slot) }
    }

    /// Get a component from the entity at `slot`
    pub fn get_mut<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRefMut<T>> {
        let storage = self.cells.get(&component.key())?.storage.borrow_mut();

        AtomicRefMut::filter_map(storage, |v| unsafe { v.get_mut(slot) })
    }

    /// Get a component from the entity at `slot`
    pub fn get_dyn(&mut self, slot: Slot, component: ComponentKey) -> Option<*mut u8> {
        let storage = self.cells.get_mut(&component)?.storage.get_mut();

        unsafe { storage.at_mut(slot) }
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        let storage = self.cells.get(&component.key())?.storage.borrow();

        // If a dummy slot is used, the archetype must have no components, so `storage.get` fails,
        // which is safe
        AtomicRef::filter_map(storage, |v| unsafe { v.get(slot) })
    }

    /// Insert a new entity into the archetype.
    /// The components must match exactly.
    ///
    /// Returns the index of the entity
    /// Entity must not exist in archetype
    #[cfg(test)]
    pub(crate) fn insert(&mut self, id: Entity, components: &mut ComponentBuffer) -> Slot {
        let slot = self.allocate(id);
        unsafe {
            for (component, src) in components.take_all() {
                let storage = self
                    .cells
                    .get_mut(&component.key)
                    .unwrap()
                    .storage
                    .get_mut();

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
        for subscriber in &self.subscribers {
            subscriber.on_spawned(id, self);
        }

        self.allocate_moved(id)
    }

    fn allocate_moved(&mut self, id: Entity) -> Slot {
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
        for subscriber in &self.subscribers {
            for &id in ids {
                subscriber.on_spawned(id, self);
            }
        }

        self.allocate_n_moved(ids)
    }
    pub(crate) fn allocate_n_moved(&mut self, ids: &[Entity]) -> Slice {
        self.reserve(ids.len());

        let last = self.len();

        self.entities.extend_from_slice(ids);

        Slice::new(last, self.len())
    }

    /// Push a type erased component into the new slot
    /// `src` shall be considered moved if Some is returned.
    /// `component` must match the type of data.
    /// # Safety
    /// Must be called only **ONCE**. Returns Err(src) if move was unsuccessful
    /// The component must be Send + Sync
    pub unsafe fn push(&mut self, component: ComponentKey, src: *mut u8, tick: u32) -> Option<()> {
        let len = self.len();
        let cell = self.cell_mut(component)?;
        let storage = cell.storage.get_mut();
        let slot = storage.len();
        assert_eq!(slot, len - 1, "Not inserting at end");
        storage.extend(src, 1);

        // TODO remove and make internal
        assert!(
            storage.len() <= len,
            "Attempt to insert more values than entities {} > {}",
            storage.len(),
            self.entities.len()
        );

        cell.changes
            .get_mut()
            .set_inserted(Change::new(Slice::single(slot), tick));

        Some(())
    }

    /// Moves the components in `storage` to the not yet initialized space in a
    /// new allocation.
    /// # Safety
    /// The length of the passed data must be equal to the slice and the slice
    /// must point to a currently uninitialized region in the archetype.
    pub(crate) unsafe fn extend(&mut self, src: &mut Storage, tick: u32) -> Option<()> {
        let len = self.len();
        let cell = self.cell_mut(src.info().key())?;
        let storage = cell.storage.get_mut();

        let additional = src.len();
        let slots = Slice::new(storage.len(), storage.len() + src.len());
        eprintln!("Pushing for slots {slots:?}");
        assert!(slots.start <= len);

        cell.storage.get_mut().append(src);
        assert!(cell.storage.get_mut().len() <= len);

        cell.changes
            .get_mut()
            .set_inserted(Change::new(slots, tick));

        Some(())
    }

    /// Move all components in `slot` to archetype of `dst`. The components not
    /// in self will be left uninitialized.
    /// # Safety
    /// `dst.put_dyn` must be called immediately after for each missing
    /// component.
    ///
    /// Returns the slot in dst and entity which was moved into current `slot`, if any.
    ///
    /// Generates change events for removed components
    pub unsafe fn move_to(
        &mut self,
        dst: &mut Self,
        slot: Slot,
        mut on_drop: impl FnMut(ComponentInfo, *mut u8),
        tick: u32,
    ) -> (Slot, Option<(Entity, Slot)>) {
        let id = self.entity(slot).expect("Invalid entity");

        let last = self.len() - 1;

        // Allocate but do not create spawn events
        let dst_slot = dst.allocate_moved(id);

        for (&key, cell) in &mut self.cells {
            // let info = cell.info;
            // let storage = cell.storage.get_mut();
            // let changes = cell.changes.get_mut();

            let dst_cell = dst.cells.get_mut(&key);
            if let Some(dst_cell) = dst_cell {
                cell.move_to(slot, dst_cell, dst_slot);
            } else {
                cell.take(slot, |info, p| (on_drop)(info, p));
                // storage.swap_remove(slot, |p| (on_drop)(&info, p));
                // changes.swap_remove(slot, last, |_, _| {});

                // // Notify the subscribers that the component was removed
                // cell.subscribers
                //     .iter()
                //     .for_each(|v| v.on_change(self, key, ChangeKind::Removed));

                // Push a removal event to dst
                dst.push_removed(key, Change::new(Slice::single(dst_slot), tick));
            }
        }

        // Make sure to carry over removed events
        for (key, removed) in &mut self.removals {
            let dst = dst.removals.entry(*key).or_default();
            removed.swap_remove_with(slot, last, |mut v| {
                v.slice = Slice::single(dst_slot);
                dst.set(v);
            })
        }

        for subscriber in &self.subscribers {
            subscriber.on_moved_from(id, self, dst);
        }

        for subscriber in &dst.subscribers {
            subscriber.on_moved_to(id, self, dst);
        }

        let swapped = self.remove_slot(slot);

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
        mut on_move: impl FnMut(ComponentInfo, *mut u8),
    ) -> Option<(Entity, Slot)> {
        let last = self.len() - 1;
        let id = self.entity(slot).expect("Invalid entity");

        for (&key, cell) in &mut self.cells {
            cell.take(slot, &mut on_move)
            // let storage = cell.storage.get_mut();
            // let info = cell.info;

            // storage.swap_remove(slot, |p| {
            //     (on_take)(info, p);
            // });

            // cell.changes.get_mut().swap_remove(slot, last, |_, _| {});

            // // Notify the subscribers that the component was removed
            // cell.subscribers
            //     .iter()
            //     .for_each(|v| v.on_change(self, key, ChangeKind::Removed));
        }

        // Remove the component removals for slot
        for removed in self.removals.values_mut() {
            removed.remove(slot, |_| {});
        }

        for subscriber in &self.subscribers {
            subscriber.on_despawned(id, self);
        }

        self.remove_slot(slot)
    }

    /// Removes the last entity
    /// Returns the popped entity id
    ///
    /// # Safety
    /// The callee is responsible to store or drop the returned components using
    /// the `on_take` function.
    pub(crate) unsafe fn pop_last(
        &mut self,
        on_take: impl FnMut(ComponentInfo, *mut u8),
    ) -> Option<Entity> {
        let last = self.last();
        if let Some(last) = last {
            self.take(self.len() - 1, on_take);
            Some(last)
        } else {
            None
        }
    }

    /// Move all entities from one archetype to another.
    ///
    /// Leaves `self` empty.
    /// Returns the new location of all entities
    pub fn move_all(&mut self, dst: &mut Self, tick: u32) -> Vec<(Entity, Slot)> {
        let len = self.len();
        // Storage is dangling
        if len == 0 {
            return Vec::new();
        }

        let entities = mem::take(&mut self.entities);

        let dst_slots = dst.allocate_n_moved(&entities);

        for (key, cell) in &mut self.cells {
            let dst_cell = dst.cells.get_mut(key);

            if let Some(dst) = dst_cell {
                assert_eq!(cell.storage.get_mut().len(), len);
                cell.move_all(dst, dst_slots.start);
                // let dst_changes = dst.changes.get_mut();

                // // Move the changes of all slots
                // for (src, dst) in self.slots().iter().zip(dst_slots) {
                //     changes.zip_map(dst_changes, |kind, a, b| {
                //         a.drain(..).for_each(|change| {
                //             change.slice.start += dst_slots.start;
                //             change.slice.end += dst_slots.start;

                //             dst_changes.set(kind, change);
                //         })
                //     })
                // }

                // // Copy this storage to the end of dst
                // unsafe { dst.storage.get_mut().append(storage) }
            } else {
                cell.clear();
                // cell.take_all()
                // storage.clear();
                // // Notify the subscribers that the component was removed
                // cell.subscribers
                //     .iter()
                //     .for_each(|v| v.on_change(self, *key, ChangeKind::Removed));

                // dst.push_removed(*key, Change::new(self.slots(), tick))
            }
        }

        // Make sure to carry over removed events
        for (key, removed) in &mut self.removals {
            let dst = dst.removals.entry(*key).or_default();
            removed.drain(..).for_each(|mut change| {
                change.slice.start += dst_slots.start;
                change.slice.end += dst_slots.start;

                dst.set(change);
            })
        }

        debug_assert_eq!(self.len(), 0);

        for subscriber in &self.subscribers {
            for &id in &entities {
                subscriber.on_moved_from(id, self, dst);
            }
        }

        for subscriber in &dst.subscribers {
            for &id in &entities {
                subscriber.on_moved_to(id, self, dst);
            }
        }

        entities.into_iter().zip_eq(dst_slots.iter()).collect_vec()
    }

    /// Reserves space for at least `additional` entities.
    /// Does nothing if the remaining capacity < additional.
    /// len remains unchanged, as does the internal order
    pub fn reserve(&mut self, additional: usize) {
        for cell in self.cells.values_mut() {
            let storage = cell.storage.get_mut();
            storage.reserve(additional);
        }
    }

    /// Returns the entity at `slot`
    pub fn entity(&self, slot: Slot) -> Option<Entity> {
        self.entities.get(slot).copied()
    }

    /// Drops all components while keeping the storage intact
    pub(crate) fn clear(&mut self) {
        for cell in self.cells.values_mut() {
            cell.storage.get_mut().clear()
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
    pub(crate) fn components(&self) -> impl Iterator<Item = ComponentInfo> + '_ {
        self.cells.values().map(|v| v.info)
    }

    /// Returns a iterator which borrows each storage in the archetype
    pub(crate) fn borrow_all(&self) -> impl Iterator<Item = AtomicRef<Storage>> {
        self.cells.values().map(|v| v.storage.borrow())
    }

    /// Access the entities in the archetype for each slot. Entity is None if
    /// the slot is not occupied, only for the last slots.
    pub fn entities(&self) -> &[Entity] {
        self.entities.as_ref()
    }

    pub(crate) fn cells(&self) -> &BTreeMap<ComponentKey, Cell> {
        &self.cells
    }

    pub(crate) fn cells_mut(&mut self) -> &mut BTreeMap<ComponentKey, Cell> {
        &mut self.cells
    }

    pub(crate) fn drain(&mut self) -> ArchetypeDrain {
        self.subscribers.iter().for_each(|v| {
            for &id in &self.entities {
                v.on_despawned(id, self);
            }
        });

        self.removals.clear();

        ArchetypeDrain {
            entities: mem::take(&mut self.entities),
            cells: mem::take(&mut self.cells),
        }
    }

    pub(crate) fn entities_mut(&mut self) -> &mut [Entity] {
        &mut self.entities
    }

    pub(crate) fn component(&self, id: ComponentKey) -> Option<ComponentInfo> {
        self.cells.get(&id).map(|v| v.info)
    }

    pub(crate) fn push_subscriber(&mut self, s: Arc<dyn Subscriber>) {
        // For component changes
        for (&key, cell) in &mut self.cells {
            if s.is_interested_component(key) {
                cell.subscribers.push(s.clone());
                cell.subscribers.retain(|v| v.is_connected())
            }
        }

        self.subscribers.push(s);
        self.subscribers.retain(|v| v.is_connected());
    }

    #[inline(always)]
    fn cell(&self, key: ComponentKey) -> Option<&Cell> {
        self.cells.get(&key)
    }

    #[inline(always)]
    fn cell_mut(&mut self, key: ComponentKey) -> Option<&mut Cell> {
        self.cells.get_mut(&key)
    }

    fn last(&self) -> Option<Entity> {
        self.entities.last().copied()
    }
}

impl Drop for Archetype {
    fn drop(&mut self) {
        self.clear();
    }
}

pub(crate) struct ArchetypeDrain {
    pub(crate) entities: Vec<Entity>,
    pub(crate) cells: BTreeMap<ComponentKey, Cell>,
}

#[derive(Clone, PartialEq, Eq, Copy)]
/// Represents a type erased component along with its memory layout and drop fn.
/// Is essentially a v-table
pub struct ComponentInfo {
    pub(crate) key: ComponentKey,
    pub(crate) layout: Layout,
    pub(crate) name: &'static str,
    pub(crate) drop: unsafe fn(*mut u8),
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    meta: fn(Self) -> ComponentBuffer,
}

impl core::fmt::Debug for ComponentInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComponentInfo")
            .field("key", &self.key)
            .field("name", &self.name)
            .finish()
    }
}

impl<T: ComponentValue> From<Component<T>> for ComponentInfo {
    fn from(v: Component<T>) -> Self {
        ComponentInfo::of(v)
    }
}

impl PartialOrd for ComponentInfo {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.key.partial_cmp(&other.key)
    }
}

impl Ord for ComponentInfo {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

impl ComponentInfo {
    /// Convert back to a typed form
    ///
    /// # Panics
    /// If the types do not match
    pub fn downcast<T: ComponentValue>(self) -> Component<T> {
        if self.type_id != TypeId::of::<T>() {
            panic!("Mismatched type");
        }

        Component::from_raw_parts(self.key, self.name, self.meta)
    }

    /// Returns the component info of a types component
    pub fn of<T: ComponentValue>(component: Component<T>) -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }
        Self {
            drop: drop_ptr::<T>,
            layout: Layout::new::<T>(),
            key: component.key(),
            name: component.name(),
            meta: component.meta(),
            type_id: TypeId::of::<T>(),
            type_name: type_name::<T>(),
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
    pub fn key(&self) -> ComponentKey {
        self.key
    }

    /// Returns the component metadata fn
    pub fn meta(&self) -> fn(ComponentInfo) -> ComponentBuffer {
        self.meta
    }

    fn align(&self) -> usize {
        self.layout.align()
    }
}

component! {
    pub(crate) unknown_component: (),
}

#[cfg(test)]
mod tests {

    use crate::{component, entity::EntityKind};
    use alloc::string::{String, ToString};
    use alloc::sync::Arc;

    use super::*;
    use core::num::NonZeroU32;

    component! {
        a: i32,
        b: String,
        c: Arc<String>,
    }

    #[test]
    pub fn test_archetype() {
        let mut arch = Archetype::new([
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
