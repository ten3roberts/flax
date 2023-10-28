use alloc::{
    collections::{btree_map, BTreeMap},
    sync::Arc,
    vec::Vec,
};
use core::{fmt::Debug, mem};

use atomic_refcell::{AtomicRef, AtomicRefCell, BorrowError, BorrowMutError};
use itertools::Itertools;

use crate::{
    component::{ComponentDesc, ComponentKey, ComponentValue},
    events::{EventData, EventKind, EventSubscriber},
    writer::ComponentUpdater,
    Component, Entity,
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
    components: Vec<ComponentDesc>,
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
    pub fn components(&self) -> &[ComponentDesc] {
        self.components.as_ref()
    }
}

pub(crate) struct CellData {
    pub(crate) storage: Storage,
    pub(crate) changes: Changes,
    subscribers: Vec<Arc<dyn EventSubscriber>>,
    pub(crate) key: ComponentKey,
}

impl CellData {
    /// Sets the specified entities and slots as modified and invokes subscribers
    /// **Note**: `ids` must be the slice of entities pointed to by `slice`
    pub(crate) fn set_modified(&mut self, ids: &[Entity], slice: Slice, change_tick: u32) {
        debug_assert_eq!(ids.len(), slice.len());
        let component = self.key;
        self.on_event(EventData {
            ids,
            key: component,
            kind: EventKind::Modified,
        });

        self.changes
            .set_modified_if_tracking(Change::new(slice, change_tick));
    }

    /// Sets the specified entities and slots as modified and invokes subscribers
    /// **Note**: `ids` must be the slice of entities pointed to by `slice`
    pub(crate) fn set_added(&mut self, ids: &[Entity], slice: Slice, change_tick: u32) {
        let component = self.key;
        self.on_event(EventData {
            ids,
            key: component,
            kind: EventKind::Added,
        });

        self.changes.set_added(Change::new(slice, change_tick));
    }
}

/// Stores a list of component values, changes, and subscribers
pub(crate) struct Cell {
    pub(crate) data: AtomicRefCell<CellData>,
    desc: ComponentDesc,
}

impl Cell {
    pub(crate) fn new(desc: ComponentDesc) -> Self {
        Self {
            data: AtomicRefCell::new(CellData {
                storage: Storage::new(desc),
                changes: Changes::new(),
                subscribers: Vec::new(),
                key: desc.key,
            }),
            desc,
        }
    }

    /// Moves a slot in the cell to another cell and slot while migrating all changes.
    fn move_to(&mut self, slot: Slot, dst: &mut Self, dst_slot: Slot) {
        let data = self.data.get_mut();

        let last = data.storage.len() - 1;

        let dst = dst.data.get_mut();

        data.storage.swap_remove(slot, |p| unsafe {
            dst.storage.extend(p, 1);
        });

        // Replace this slot with the last slot and move everything to the dst archetype
        data.changes.swap_remove(slot, last, |kind, v| {
            dst.changes.set_slot(kind, dst_slot, v.tick);
        });

        // Do not notify of removal, since the component is still intact, but in another archetype
    }

    /// Moves all slots to another cell
    fn move_all(&mut self, dst: &mut Self, dst_start: Slot) {
        let data = self.data.get_mut();

        let dst = dst.data.get_mut();

        debug_assert_eq!(dst.storage.len(), dst_start);
        unsafe { dst.storage.append(&mut data.storage) }

        data.changes.zip_map(&mut dst.changes, |_, a, b| {
            a.inner.drain(..).for_each(|mut change| {
                change.slice.start += dst_start;
                change.slice.end += dst_start;

                b.set(change);
            })
        });
    }

    /// Move a slot out of the cell by swapping with the last
    fn take(&mut self, slot: Slot, mut on_move: impl FnMut(ComponentDesc, *mut u8)) {
        let data = self.data.get_mut();

        let last = data.storage.len() - 1;

        data.storage.swap_remove(slot, |p| on_move(self.desc, p));
        data.changes.swap_remove(slot, last, |_, _| {});
    }

    /// Silently clears (and drops) all components and changes.
    fn clear(&mut self) {
        let data = self.data.get_mut();

        data.storage.clear();
        data.changes.clear();
    }

    /// Drain the values in the cell.
    pub(crate) fn drain(&mut self) -> Storage {
        let data = self.data.get_mut();
        let storage = mem::replace(&mut data.storage, Storage::new(self.desc));
        data.changes.clear();

        storage
    }

    /// # Safety
    ///
    /// Assumes `self` is of type `T`
    pub(crate) unsafe fn get<T: ComponentValue>(&self, slot: Slot) -> Option<AtomicRef<T>> {
        let data = self.data.borrow();
        AtomicRef::filter_map(data, |v| v.storage.downcast_ref::<T>().get(slot))
    }

    /// # Safety
    ///
    /// Assumes `self` is of type `T`
    pub(crate) unsafe fn try_get<T: ComponentValue>(
        &self,
        slot: Slot,
    ) -> Result<Option<AtomicRef<T>>, BorrowError> {
        let data = self.data.try_borrow()?;
        Ok(AtomicRef::filter_map(data, |v| {
            v.storage.downcast_ref::<T>().get(slot)
        }))
    }

    #[inline]
    pub fn borrow<T: ComponentValue>(&self) -> CellGuard<[T]> {
        CellGuard::new(self.data.borrow())
    }

    #[inline]
    pub fn borrow_mut<T: ComponentValue>(&self) -> CellMutGuard<[T]> {
        CellMutGuard::new(self.data.borrow_mut())
    }

    // #[inline]
    // pub fn try_borrow<T: ComponentValue>(&self) -> Result<CellGuard<[T]>, BorrowError> {
    //     Ok(CellGuard::new(self.data.try_borrow()?))
    // }

    // #[inline]
    // pub fn try_borrow_mut<T: ComponentValue>(&self) -> Result<CellMutGuard<[T]>, BorrowMutError> {
    //     Ok(CellMutGuard::new(self.data.try_borrow_mut()?))
    // }

    #[inline]
    pub fn get_mut<T: ComponentValue>(
        &self,
        id: Entity,
        slot: Slot,
        tick: u32,
    ) -> Option<RefMut<T>> {
        RefMut::new(self.borrow_mut(), id, slot, tick)
    }
}

// #[derive(Debug)]
#[doc(hidden)]
/// A collection of entities with the same components.
/// Stored as columns of contiguous component data.
pub struct Archetype {
    cells: BTreeMap<ComponentKey, Cell>,
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

impl CellData {
    #[inline]
    fn on_event(&self, event: EventData) {
        for handler in self.subscribers.iter() {
            handler.on_event(&event)
        }
    }
}

impl Archetype {
    pub(crate) fn empty() -> Self {
        Self {
            cells: BTreeMap::new(),
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
        I: IntoIterator<Item = ComponentDesc>,
    {
        let cells = components
            .into_iter()
            .map(|desc| {
                let key = desc.key();

                (key, Cell::new(desc))
            })
            .collect();

        Self {
            cells,
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
        debug_assert!(self.has(component), "Archetype has the incoming component");

        let existing = self.incoming.insert(component, dst_id);

        debug_assert!(
            existing.is_none() || existing == Some(dst_id),
            "Insert incoming for {component:?} => {dst_id}. Existing: {existing:?}"
        )
    }

    pub(crate) fn add_outgoing(&mut self, component: ComponentKey, dst_id: ArchetypeId) {
        self.outgoing.insert(component, dst_id);
    }

    pub(crate) fn add_child(&mut self, component: ComponentKey, id: ArchetypeId) {
        self.outgoing.insert(component, id);

        let existing = self.children.insert(component, id);
        debug_assert!(existing.is_none());
    }

    pub(crate) fn borrow<T: ComponentValue>(
        &self,
        component: ComponentKey,
    ) -> Option<CellGuard<[T]>> {
        Some(self.cell(component)?.borrow())
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
        component: ComponentKey,
    ) -> Option<CellMutGuard<[T]>> {
        let cell = self.cell(component)?;
        let data = cell.borrow_mut();
        Some(data)
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
    pub fn desc(&self) -> ArchetypeInfo {
        let (components, storage) = self
            .cells
            .values()
            .map(|v| {
                let data = v.data.borrow();
                (
                    v.desc,
                    StorageInfo {
                        cap: data.storage.capacity(),
                        len: data.storage.len(),
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

    /// Get a component from the entity at `slot`
    pub(crate) fn get_mut<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
        tick: u32,
    ) -> Option<RefMut<T>> {
        self.cell(component.key())?
            .get_mut(self.entities[slot], slot, tick)
    }

    /// Get a component from the entity at `slot`
    pub(crate) fn try_get_mut<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
        tick: u32,
    ) -> Result<Option<RefMut<T>>, BorrowMutError> {
        let cell = match self.cell(component.key()) {
            Some(v) => v,
            None => return Ok(None),
        };

        Ok(cell.get_mut(self.entities[slot], slot, tick))
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

        let data = cell.data.get_mut();

        let value = unsafe { data.storage.at_mut(slot)? };
        let value = (modify)(value);

        data.set_modified(
            &self.entities[slot..=slot],
            Slice::single(slot),
            change_tick,
        );

        Some(value)
    }

    /// Get a component from the entity at `slot`
    #[inline]
    pub(crate) fn update<T: ComponentValue, U: ComponentUpdater>(
        &self,
        slot: Slot,
        component: Component<T>,
        writer: U,
        tick: u32,
    ) -> Option<U::Updated> {
        let cell = self.cells.get(&component.key())?;

        let mut data = cell.data.borrow_mut();

        let res = unsafe { writer.update(&mut data, slot, self.entities[slot], tick) };

        Some(res)
    }

    /// Get a component from the entity at `slot`.
    pub(crate) fn get<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Option<AtomicRef<T>> {
        let cell = self.cell(component.key())?;
        unsafe { cell.get(slot) }
    }

    /// Get a component from the entity at `slot`.
    pub(crate) fn try_get<T: ComponentValue>(
        &self,
        slot: Slot,
        component: Component<T>,
    ) -> Result<Option<AtomicRef<T>>, BorrowError> {
        let cell = match self.cell(component.key()) {
            Some(v) => v,
            None => return Ok(None),
        };
        unsafe { cell.try_get(slot) }
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
        for (desc, src) in buffer.drain() {
            unsafe {
                let data = self.cells.get_mut(&desc.key).unwrap().data.get_mut();

                data.storage.extend(src, 1);
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
    /// `component` must match the type of data.
    /// # Safety
    /// Must be called only **ONCE**. Returns Err(src) if move was unsuccessful
    /// The component must be Send + Sync
    pub unsafe fn push(&mut self, component: ComponentKey, src: *mut u8, tick: u32) {
        let len = self.len();
        let cell = self.cells.get_mut(&component).unwrap();
        let data = cell.data.get_mut();

        let slot = data.storage.len();
        assert_eq!(slot, len - 1, "Not inserting at end");
        data.storage.extend(src, 1);

        // TODO remove and make internal
        assert!(
            data.storage.len() <= len,
            "Attempt to insert more values than entities {} > {}",
            data.storage.len(),
            self.entities.len()
        );

        data.set_added(&self.entities[slot..=slot], Slice::single(slot), tick);
    }

    /// Moves the components in `storage` to the not yet initialized space in a
    /// new allocation.
    /// # Safety
    /// The length of the passed data must be equal to the slice and the slice
    /// must point to a currently uninitialized region in the archetype.
    pub(crate) unsafe fn extend(&mut self, src: &mut Storage, tick: u32) {
        if src.is_empty() {
            return;
        }

        let len = self.len();
        let cell = self.cells.get_mut(&src.desc().key()).unwrap();
        let data = cell.data.get_mut();

        let slots = Slice::new(data.storage.len(), data.storage.len() + src.len());
        debug_assert!(slots.start <= len);

        data.storage.append(src);
        debug_assert!(data.storage.len() <= len);

        data.set_added(&self.entities[slots.as_range()], slots, tick);
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
        mut on_drop: impl FnMut(ComponentDesc, *mut u8),
    ) -> (Slot, Option<(Entity, Slot)>) {
        let id = self.entity(slot).expect("Invalid entity");

        let dst_slot = dst.allocate(id);

        for (&key, cell) in &mut self.cells {
            let data = cell.data.get_mut();

            let dst_cell = dst.cells.get_mut(&key);
            if let Some(dst_cell) = dst_cell {
                cell.move_to(slot, dst_cell, dst_slot);
            } else {
                // Notify the subscribers that the component was removed
                data.on_event(EventData {
                    ids: &[id],
                    key,
                    kind: EventKind::Removed,
                });

                cell.take(slot, &mut on_drop);
            }
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
    /// TODO: test with change query
    pub unsafe fn take(
        &mut self,
        slot: Slot,
        mut on_move: impl FnMut(ComponentDesc, *mut u8),
    ) -> Option<(Entity, Slot)> {
        let id = self.entity(slot).expect("Invalid entity");

        // for subscriber in &self.subscribers {
        //     subscriber.on_despawned(id, slot, self);
        // }

        for cell in self.cells.values_mut() {
            let data = cell.data.get_mut();
            // data.on_event(&self.entities, Slice::single(slot), EventKind::Removed);
            data.on_event(EventData {
                ids: &[id],
                key: data.key,
                kind: EventKind::Removed,
            });

            cell.take(slot, &mut on_move)
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
        on_take: impl FnMut(ComponentDesc, *mut u8),
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
    pub fn move_all(&mut self, dst: &mut Self) -> Vec<(Entity, Slot)> {
        let len = self.len();
        if len == 0 {
            return Vec::new();
        }

        let slots = self.slots();
        let entities = mem::take(&mut self.entities);

        let dst_slots = dst.allocate_n(&entities);

        for (key, cell) in &mut self.cells {
            let data = cell.data.get_mut();

            let dst_cell = dst.cells.get_mut(key);

            if let Some(dst) = dst_cell {
                assert_eq!(data.storage.len(), len);
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
                data.on_event(EventData {
                    ids: &entities[slots.as_range()],
                    key: data.key,
                    kind: EventKind::Removed,
                });

                cell.clear();
            }
        }

        debug_assert_eq!(self.len(), 0);

        entities.into_iter().zip_eq(dst_slots.iter()).collect_vec()
    }

    /// Reserves space for at least `additional` entities.
    /// Does nothing if the remaining capacity < additional.
    /// len remains unchanged, as does the internal order
    pub fn reserve(&mut self, additional: usize) {
        for cell in self.cells.values_mut() {
            let data = cell.data.get_mut();
            data.storage.reserve(additional);
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
            let data = cell.data.get_mut();
            // Notify the subscribers that the component was removed
            // data.on_event(&self.entities, slots, EventKind::Removed);
            data.on_event(EventData {
                ids: &self.entities[slots.as_range()],
                key: data.key,
                kind: EventKind::Removed,
            });

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
    pub(crate) fn components(&self) -> impl Iterator<Item = ComponentDesc> + '_ {
        self.cells.values().map(|v| v.desc)
    }

    #[allow(dead_code)]
    pub(crate) fn component_names(&self) -> impl Iterator<Item = &str> {
        self.cells.values().map(|v| v.desc.name())
    }

    /// Returns a iterator which attempts to borrows each storage in the archetype
    pub(crate) fn try_borrow_all(&self) -> impl Iterator<Item = Option<AtomicRef<CellData>>> {
        self.cells.values().map(|v| v.data.try_borrow().ok())
    }
    /// Access the entities in the archetype for each slot. Entity is None if
    /// the slot is not occupied, only for the last slots.
    #[inline]
    pub fn entities(&self) -> &[Entity] {
        self.entities.as_ref()
    }

    pub(crate) fn cells(&self) -> &BTreeMap<ComponentKey, Cell> {
        &self.cells
    }

    pub(crate) fn drain(&mut self) -> ArchetypeDrain {
        let slots = self.slots();
        for cell in self.cells.values_mut() {
            let data = cell.data.get_mut();
            data.on_event(EventData {
                ids: &self.entities[slots.as_range()],
                key: data.key,
                kind: EventKind::Removed,
            })
        }

        ArchetypeDrain {
            entities: mem::take(&mut self.entities),
            cells: mem::take(&mut self.cells),
        }
    }

    pub(crate) fn entities_mut(&mut self) -> &mut [Entity] {
        &mut self.entities
    }

    pub(crate) fn component(&self, key: ComponentKey) -> Option<ComponentDesc> {
        self.cell(key).map(|v| v.desc)
    }

    /// Add a new subscriber. The subscriber must be interested in this archetype
    pub(crate) fn add_handler(&mut self, s: Arc<dyn EventSubscriber>) {
        // For component changes
        for cell in self.cells.values_mut() {
            let data = cell.data.get_mut();
            if s.matches_component(cell.desc) {
                data.subscribers.push(s.clone());
            }

            data.subscribers.retain(|v| v.is_connected())
        }
    }

    #[inline(always)]
    pub(crate) fn cell(&self, key: ComponentKey) -> Option<&Cell> {
        self.cells.get(&key)
    }

    #[inline(always)]
    pub(crate) fn cell_mut(&mut self, key: ComponentKey) -> Option<&mut Cell> {
        self.cells.get_mut(&key)
    }

    fn last(&self) -> Option<Entity> {
        self.entities.last().copied()
    }

    pub(crate) fn remove_link(&mut self, component: ComponentKey) {
        let linked = self.outgoing.remove(&component);
        assert!(linked.is_some());

        self.children.remove(&component);
    }

    /// Borrow the change list mutably
    #[cfg(test)]
    pub(crate) fn changes_mut(&mut self, component: ComponentKey) -> Option<&mut Changes> {
        Some(&mut self.cell_mut(component)?.data.get_mut().changes)
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
            ComponentDesc::of(a()),
            ComponentDesc::of(b()),
            ComponentDesc::of(c()),
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
