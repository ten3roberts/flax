use alloc::{
    collections::{btree_map, BTreeMap},
    sync::Arc,
    vec::Vec,
};
use core::{fmt::Debug, mem};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use itertools::Itertools;

use crate::{
    events::{EventData, EventKind, EventSubscriber},
    Component, ComponentInfo, ComponentKey, ComponentValue, Entity,
};

/// Unique archetype id
pub type ArchetypeId = Entity;
/// Represents a slot in the archetype
pub type Slot = usize;

mod batch;
mod changes;
mod guard;
mod slice;
mod storage;

pub use batch::*;
pub use changes::*;
pub use slice::*;
pub(crate) use storage::*;

pub use guard::*;

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

/// Human friendly archetype inspection
#[derive(Default, Clone)]
pub struct ArchetypeInfo {
    storage: Vec<StorageInfo>,
    components: Vec<ComponentInfo>,
    entities: usize,
}

impl Debug for ArchetypeInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ArchetypeInfo")
            .field("components", &self.components)
            .field("entities", &self.entities)
            .finish()
    }
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
}

/// Stores a list of component values, changes, and subscribers
pub(crate) struct Cell {
    storage: AtomicRefCell<Storage>,
    changes: AtomicRefCell<Changes>,
    info: ComponentInfo,
    pub(crate) subscribers: Vec<Arc<dyn EventSubscriber>>,
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

        let dst_storage = dst.storage.get_mut();
        let dst_changes = dst.changes.get_mut();

        debug_assert_eq!(dst_storage.len(), dst_start);
        unsafe { dst_storage.append(storage) }

        changes.zip_map(dst_changes, |_, a, b| {
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
    }

    /// Silently clears (and drops) all components and changes.
    fn clear(&mut self) {
        let storage = self.storage.get_mut();
        let changes = self.changes.get_mut();
        // if !storage.is_empty() {
        //     // Notify removed
        //     for v in self.subscribers.iter() {
        //         v.on_change(self.info, ChangeKind::Removed)
        //     }
        // }

        storage.clear();
        changes.clear();

        // Notify subscribers
    }

    /// Drain the values in the cell.
    pub(crate) fn drain(&mut self) -> Storage {
        let storage = mem::replace(self.storage.get_mut(), Storage::new(self.info));
        self.changes.get_mut().clear();

        // Notify subscribers
        storage
    }

    pub(crate) fn storage(&self) -> &AtomicRefCell<Storage> {
        &self.storage
    }

    pub(crate) unsafe fn get<T: ComponentValue>(&self, slot: Slot) -> Option<AtomicRef<T>> {
        let storage = self.storage.borrow();
        AtomicRef::filter_map(storage, |v| v.downcast_ref::<T>().get(slot))
    }

    #[inline]
    pub fn borrow_mut<'a, T: ComponentValue>(
        &'a self,
        entities: &'a [Entity],
        tick: u32,
    ) -> CellMutGuard<'a, [T]> {
        let storage = self.storage.borrow_mut();
        let changes = self.changes.borrow_mut();

        CellMutGuard {
            storage: AtomicRefMut::map(storage, |v| v.downcast_mut()),
            changes,
            cell: self,
            ids: entities,
            tick,
        }
    }

    pub(crate) unsafe fn get_mut<'a, T: ComponentValue>(
        &'a self,
        entities: &'a [Entity],
        slot: Slot,
        tick: u32,
    ) -> Option<RefMut<'a, T>> {
        RefMut::new(self.borrow_mut(entities, tick), slot)
    }

    // pub(crate) fn info(&self) -> ComponentInfo {
    //     self.info
    // }

    #[inline]
    pub(crate) fn info(&self) -> ComponentInfo {
        self.info
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
    pub(crate) children: BTreeMap<ComponentKey, ArchetypeId>,
    pub(crate) outgoing: BTreeMap<ComponentKey, ArchetypeId>,
    pub(crate) incoming: BTreeMap<ComponentKey, ArchetypeId>,
}

/// Since all components are Send + Sync, the cells are as well
unsafe impl Send for Cell {}
unsafe impl Sync for Cell {}

impl Cell {
    fn on_event(&mut self, all_ids: &[Entity], slots: Slice, kind: EventKind) {
        let event = EventData {
            ids: &all_ids[slots.as_range()],
            key: self.info.key,
            kind,
        };

        for handler in self.subscribers.iter() {
            handler.on_event(&event)
        }
    }
}

impl Archetype {
    pub(crate) fn empty() -> Self {
        Self {
            cells: BTreeMap::new(),
            removals: BTreeMap::new(),
            incoming: BTreeMap::new(),
            entities: Vec::new(),
            children: Default::default(),
            outgoing: Default::default(),
        }
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
                        changes: AtomicRefCell::new(Changes::new()),
                        subscribers: Vec::new(),
                    },
                )
            })
            .collect();

        Self {
            cells,
            removals: BTreeMap::new(),
            incoming: BTreeMap::new(),
            entities: Vec::new(),
            children: Default::default(),
            outgoing: Default::default(),
        }
    }

    /// Returns all the relation components in the archetype
    pub fn relations(&self) -> impl Iterator<Item = ComponentKey> + '_ {
        self.cells.keys().filter(|v| v.is_relation()).copied()
    }

    pub(crate) fn relations_like(&self, relation: Entity) -> btree_map::Range<ComponentKey, Cell> {
        self.cells.range(
            ComponentKey::new(relation, Some(Entity::MIN))
                ..=ComponentKey::new(relation, Some(Entity::MAX)),
        )
    }

    /// Returns all the slots in the archetype
    pub fn slots(&self) -> Slice {
        Slice::new(0, self.len())
    }

    /// Returns true if the archtype has `component`
    pub fn has(&self, component: ComponentKey) -> bool {
        self.cells.contains_key(&component)
    }

    pub(crate) fn incoming(&self, component: ComponentKey) -> Option<ArchetypeId> {
        self.incoming.get(&component).copied()
    }

    pub(crate) fn add_incoming(&mut self, component: ComponentKey, dst_id: ArchetypeId) {
        self.incoming.insert(component, dst_id);
    }

    pub(crate) fn add_outgoing(&mut self, component: ComponentKey, dst_id: ArchetypeId) {
        self.outgoing.insert(component, dst_id);
    }

    pub(crate) fn add_child(&mut self, component: ComponentKey, id: ArchetypeId) {
        self.outgoing.insert(component, id);

        let existing = self.children.insert(component, id);
        debug_assert!(existing.is_none());
    }

    fn push_removed(&mut self, key: ComponentKey, change: Change) {
        self.removals.entry(key).or_default().set(change);
    }

    pub(crate) fn borrow<T: ComponentValue>(
        &self,
        component: ComponentKey,
    ) -> Option<AtomicRef<[T]>> {
        let storage = self.cell(component)?.storage.borrow();
        Some(AtomicRef::map(storage, |v| unsafe { v.borrow() }))
    }

    /// Access a component storage mutably.
    ///
    /// Return a reference to the change list, which must be used to push the slots which where
    /// modified.
    ///
    /// # Panics
    /// If the storage or changes is already borrowed
    pub(crate) fn borrow_mut<T: ComponentValue>(
        &self,
        component: Component<T>,
        tick: u32,
    ) -> Option<CellMutGuard<[T]>> {
        Some(self.cell(component.key())?.borrow_mut(&self.entities, tick))
    }

    /// Removes a slot and swaps in the last slot
    #[inline(always)]
    unsafe fn remove_slot(&mut self, slot: Slot) -> Option<(Entity, Slot)> {
        let last = self.len() - 1;
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
            entities: self.entities.len(),
        }
    }

    /// Borrow the change list
    pub(crate) fn changes(&self, component: ComponentKey) -> Option<AtomicRef<Changes>> {
        let changes = self.cell(component)?.changes.borrow();
        Some(changes)
    }

    pub(crate) fn removals(&self, component: ComponentKey) -> Option<&ChangeList> {
        self.removals.get(&component)
    }

    /// Borrow the change list mutably
    #[cfg(test)]
    pub(crate) fn changes_mut(&self, component: ComponentKey) -> Option<AtomicRefMut<Changes>> {
        let changes = self.cell(component)?.changes.borrow_mut();
        Some(changes)
    }

    /// Get a component from the entity at `slot`
    pub(crate) fn get_mut<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
        tick: u32,
    ) -> Option<RefMut<T>> {
        let cell = self.cell(component.key())?;

        unsafe { cell.get_mut(&self.entities, slot, tick) }
    }

    /// Get a component from the entity at `slot`
    #[inline]
    pub fn mutate_in_place<T>(
        &mut self,
        slot: Slot,
        component: ComponentKey,
        change_tick: u32,
        modify: impl FnOnce(*mut u8) -> T,
    ) -> Option<T> {
        let cell = self.cells.get_mut(&component)?;

        let value = unsafe { cell.storage.get_mut().at_mut(slot)? };
        let value = (modify)(value);

        cell.on_event(&self.entities, Slice::single(slot), EventKind::Modified);

        cell.changes
            .get_mut()
            .set_modified_if_tracking(Change::new(Slice::single(slot), change_tick));

        Some(value)
    }

    /// Get a component from the entity at `slot`. Assumes slot is valid.
    pub fn get<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        let cell = self.cell(component.key())?;
        unsafe { cell.get(slot) }
    }

    /// Insert a new entity into the archetype.
    /// The components must match exactly.
    ///
    /// Returns the index of the entity
    /// Entity must not exist in archetype
    #[cfg(test)]
    pub(crate) fn insert(
        &mut self,
        id: Entity,
        buffer: &mut crate::buffer::ComponentBuffer,
    ) -> Slot {
        let slot = self.allocate(id);
        for (info, src) in buffer.drain() {
            unsafe {
                let storage = self.cells.get_mut(&info.key).unwrap().storage.get_mut();

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
        let cell = self.cells.get_mut(&component)?;
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

        cell.on_event(&self.entities, Slice::single(slot), EventKind::Added);

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
        let cell = self.cells.get_mut(&src.info().key())?;
        let storage = cell.storage.get_mut();

        let slots = Slice::new(storage.len(), storage.len() + src.len());
        debug_assert!(slots.start <= len);

        cell.storage.get_mut().append(src);
        debug_assert!(cell.storage.get_mut().len() <= len);

        cell.on_event(&self.entities, slots, EventKind::Added);

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

        let dst_slot = dst.allocate(id);

        // // Before the cells
        // for subscriber in &self.subscribers {
        //     subscriber.on_moved_pre(id, slot, self, dst);
        // }

        for (&key, cell) in &mut self.cells {
            // let info = cell.info;
            // let storage = cell.storage.get_mut();
            // let changes = cell.changes.get_mut();

            let dst_cell = dst.cells.get_mut(&key);
            if let Some(dst_cell) = dst_cell {
                cell.move_to(slot, dst_cell, dst_slot);
            } else {
                // Notify the subscribers that the component was removed
                cell.on_event(&self.entities, Slice::single(slot), EventKind::Removed);

                cell.take(slot, &mut on_drop);
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

        // for subscriber in &dst.subscribers {
        //     subscriber.on_moved_post(id, self, dst);
        // }

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
    /// TODO: test with change query
    pub unsafe fn take(
        &mut self,
        slot: Slot,
        mut on_move: impl FnMut(ComponentInfo, *mut u8),
    ) -> Option<(Entity, Slot)> {
        self.entity(slot).expect("Invalid entity");

        // for subscriber in &self.subscribers {
        //     subscriber.on_despawned(id, slot, self);
        // }

        for cell in self.cells.values_mut() {
            cell.on_event(&self.entities, Slice::single(slot), EventKind::Removed);

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
        if len == 0 {
            return Vec::new();
        }

        let slots = self.slots();
        let entities = mem::take(&mut self.entities);

        let dst_slots = dst.allocate_n(&entities);

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
                // Notify the subscribers that the component was removed
                cell.on_event(&entities, slots, EventKind::Removed);

                cell.clear();
                dst.push_removed(*key, Change::new(dst_slots, tick))
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

    /// Drops all components and changes.
    pub(crate) fn clear(&mut self) {
        let slots = self.slots();
        for cell in self.cells.values_mut() {
            // Notify the subscribers that the component was removed
            cell.on_event(&self.entities, slots, EventKind::Removed);
            cell.clear()
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

    #[allow(dead_code)]
    pub(crate) fn component_names(&self) -> impl Iterator<Item = &str> {
        self.cells.values().map(|v| v.info.name())
    }

    /// Returns a iterator which attempts to borrows each storage in the archetype
    pub(crate) fn try_borrow_all(&self) -> impl Iterator<Item = Option<AtomicRef<Storage>>> {
        self.cells.values().map(|v| v.storage.try_borrow().ok())
    }
    /// Access the entities in the archetype for each slot. Entity is None if
    /// the slot is not occupied, only for the last slots.
    pub fn entities(&self) -> &[Entity] {
        self.entities.as_ref()
    }

    pub(crate) fn cells(&self) -> &BTreeMap<ComponentKey, Cell> {
        &self.cells
    }

    pub(crate) fn drain(&mut self) -> ArchetypeDrain {
        let slots = self.slots();
        for cell in self.cells.values_mut() {
            cell.on_event(&self.entities, slots, EventKind::Removed);
        }

        self.removals.clear();

        ArchetypeDrain {
            entities: mem::take(&mut self.entities),
            cells: mem::take(&mut self.cells),
        }
    }

    pub(crate) fn entities_mut(&mut self) -> &mut [Entity] {
        &mut self.entities
    }

    pub(crate) fn component(&self, key: ComponentKey) -> Option<ComponentInfo> {
        self.cell(key).map(|v| v.info)
    }

    /// Add a new subscriber. The subscriber must be interested in this archetype
    pub(crate) fn add_handler(&mut self, s: Arc<dyn EventSubscriber>) {
        // For component changes
        for cell in self.cells.values_mut() {
            if s.matches_component(cell.info) {
                cell.subscribers.push(s.clone());
            }

            cell.subscribers.retain(|v| v.is_connected())
        }
    }

    #[inline(always)]
    pub(crate) fn cell(&self, key: ComponentKey) -> Option<&Cell> {
        self.cells.get(&key)
    }

    fn last(&self) -> Option<Entity> {
        self.entities.last().copied()
    }

    pub(crate) fn remove_link(&mut self, component: ComponentKey) {
        let linked = self.outgoing.remove(&component);
        self.children.remove(&component);
        assert!(linked.is_some());
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

#[cfg(test)]
mod tests {

    use crate::buffer::ComponentBuffer;
    use crate::entity::DEFAULT_GEN;
    use crate::{component, entity::EntityKind};
    use alloc::string::{String, ToString};
    use alloc::sync::Arc;

    use super::*;

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

        let id = Entity::from_parts(6, DEFAULT_GEN.saturating_add(1), EntityKind::empty());
        let id_2 = Entity::from_parts(5, DEFAULT_GEN.saturating_add(1), EntityKind::empty());

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

        arch.get_mut(slot, b(), 0).unwrap().push_str("Bar");

        assert_eq!(arch.get(slot, b()).as_deref(), Some(&"FooBar".to_string()));
        assert_eq!(arch.entity(slot), Some(id));
        assert_eq!(arch.entity(slot_2), Some(id_2));

        drop(arch);

        assert_eq!(Arc::strong_count(&shared), 1);
    }
}
