use core::{mem, ptr, slice};

use alloc::vec::Vec;
use itertools::{Either, Itertools};

use crate::{
    archetype::{ArchetypeId, CellData, Slice, Slot},
    buffer::ComponentBuffer,
    component::{ComponentDesc, ComponentValue},
    entity::EntityLocation,
    metadata::exclusive,
    world::update_entity_loc,
    Entity, World,
};

/// Describes a modification to the components of an entity within the context of an archetype
pub(crate) trait ComponentUpdater {
    type Updated;
    /// Performs write operations against the target entity
    /// # Safety
    ///
    /// The provided `data` must be of the same type as the cell data and what will be
    /// written using `self`
    unsafe fn update(self, data: &mut CellData, slot: Slot, id: Entity, tick: u32)
        -> Self::Updated;
}

pub(crate) trait ComponentPusher {
    type Pushed;
    /// # Safety
    ///
    /// The cell **must** be extended with valid component data for the new entity.
    ///
    /// The type of `data` must match that of `self`
    unsafe fn push(self, data: &mut CellData, id: Entity, tick: u32) -> Self::Pushed;
}

pub(crate) struct FnWriter<F, T> {
    func: F,
    _marker: core::marker::PhantomData<T>,
}

impl<F, T> FnWriter<F, T> {
    pub(crate) fn new(func: F) -> Self {
        Self {
            func,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<F, T, U> ComponentUpdater for FnWriter<F, T>
where
    F: FnOnce(&mut T) -> U,
{
    type Updated = U;

    unsafe fn update(self, data: &mut CellData, slot: Slot, id: Entity, tick: u32) -> U {
        let value = &mut *(data.storage.at_mut(slot).unwrap() as *mut T);
        let res = (self.func)(value);

        data.set_modified(&[id], Slice::single(slot), tick);
        res
    }
}

/// # Safety
///
/// The entity must be fully initialized and all bookkepping updated
pub unsafe trait EntityWriter {
    type Output;
    fn write(
        self,
        world: &mut World,
        id: Entity,
        loc: EntityLocation,
        tick: u32,
    ) -> (EntityLocation, Self::Output);
}

pub(crate) struct SingleComponentWriter<W> {
    desc: ComponentDesc,
    writer: W,
}

impl<W> SingleComponentWriter<W> {
    pub(crate) fn new(desc: ComponentDesc, writer: W) -> Self {
        Self { desc, writer }
    }
}

unsafe impl<W: ComponentUpdater + ComponentPusher> EntityWriter for SingleComponentWriter<W> {
    type Output = Either<W::Updated, W::Pushed>;

    fn write(
        self,
        world: &mut World,
        id: Entity,
        src_loc: EntityLocation,
        tick: u32,
    ) -> (EntityLocation, Self::Output) {
        let key = self.desc.key();

        let arch = world.archetypes.get_mut(src_loc.arch_id);

        if let Some(cell) = arch.cell_mut(key) {
            let res = unsafe {
                self.writer
                    .update(cell.data.get_mut(), src_loc.slot, id, tick)
            };

            return (src_loc, Either::Left(res));
        }

        let (src, dst, dst_id) = if let Some(&dst_id) = arch.outgoing.get(&key) {
            let (src, dst) = world
                .archetypes
                .get_disjoint(src_loc.arch_id, dst_id)
                .unwrap();
            (src, dst, dst_id)
        } else {
            // Oh no! The archetype is missing the component
            let exclusive = if self.desc.meta_ref().has(exclusive()) {
                slice::from_ref(&self.desc.key.id)
            } else {
                &[]
            };

            let (components, superset) =
                find_archetype_components(arch.components(), [self.desc], exclusive);

            world.init_component(self.desc);
            let (dst_id, _) = world.archetypes.find_create(components.iter().copied());

            // Add a quick edge to refer to later
            let reserved_id = world.archetypes.reserved;
            let (src, dst) = world
                .archetypes
                .get_disjoint(src_loc.arch_id, dst_id)
                .unwrap();

            if superset && src_loc.arch_id != reserved_id {
                src.add_outgoing(key, dst_id);
                dst.add_incoming(key, src_loc.arch_id);
            }

            (src, dst, dst_id)
        };

        let (dst_slot, swapped) = unsafe { src.move_to(dst, src_loc.slot, |c, ptr| c.drop(ptr)) };

        // Insert the missing component
        let pushed = unsafe {
            let cell = dst
                .cell_mut(key)
                .expect("Missing component in new archetype");

            let data = cell.data.get_mut();

            self.writer.push(data, id, tick)
        };

        let dst_loc = EntityLocation {
            arch_id: dst_id,
            slot: dst_slot,
        };

        update_entity_loc(world, id, dst_loc, swapped);

        (dst_loc, Either::Right(pushed))
    }
}

/// # Safety
/// *All* components of the new slot must be initialized
pub(crate) unsafe trait MigrateEntity {
    fn migrate(
        self,
        world: &mut World,
        src_id: ArchetypeId,
        src_slot: Slot,
        tick: u32,
    ) -> (EntityLocation, Option<(Entity, Slot)>);
}

pub(crate) struct Replace<T: ComponentValue> {
    pub(crate) value: T,
}

impl<T: ComponentValue> Replace<T> {
    pub(crate) fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T: ComponentValue> ComponentUpdater for Replace<T> {
    type Updated = T;

    unsafe fn update(self, data: &mut CellData, slot: Slot, id: Entity, tick: u32) -> T {
        let storage = data.storage.downcast_mut::<T>();
        let old = mem::replace(&mut storage[slot], self.value);

        data.set_modified(&[id], Slice::single(slot), tick);

        old
    }
}

impl<T: ComponentValue> ComponentPusher for Replace<T> {
    type Pushed = ();

    unsafe fn push(mut self, data: &mut CellData, id: Entity, tick: u32) {
        let slot = data.storage.len();

        data.storage.extend(&mut self.value as *mut T as *mut u8, 1);

        mem::forget(self.value);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct Missing<T: ComponentValue> {
    pub(crate) value: T,
}

impl<T: ComponentValue> ComponentUpdater for Missing<T> {
    type Updated = ();

    unsafe fn update(self, _: &mut CellData, _: Slot, _: Entity, _: u32) {}
}

impl<T: ComponentValue> ComponentPusher for Missing<T> {
    type Pushed = ();

    unsafe fn push(mut self, data: &mut CellData, id: Entity, tick: u32) {
        let slot = data.storage.len();

        data.storage.extend(&mut self.value as *mut T as *mut u8, 1);

        mem::forget(self.value);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct WriteDedup<T: ComponentValue> {
    pub(crate) value: T,
}

impl<T: ComponentValue> WriteDedup<T> {
    pub(crate) fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T: ComponentValue + PartialEq> ComponentUpdater for WriteDedup<T> {
    type Updated = ();

    unsafe fn update(self, data: &mut CellData, slot: Slot, id: Entity, tick: u32) {
        let storage = data.storage.downcast_mut::<T>();
        let current = &mut storage[slot];
        if current != &self.value {
            *current = self.value;

            data.set_modified(&[id], Slice::single(slot), tick);
        }
    }
}

impl<T: ComponentValue + PartialEq> ComponentPusher for WriteDedup<T> {
    type Pushed = ();

    unsafe fn push(mut self, data: &mut CellData, id: Entity, tick: u32) {
        let slot = data.storage.len();

        data.storage.extend(&mut self.value as *mut T as *mut u8, 1);

        mem::forget(self.value);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct WriteDedupDyn {
    pub(crate) value: *mut u8,
    pub(crate) cmp: unsafe fn(*const u8, *const u8) -> bool,
}

impl ComponentUpdater for WriteDedupDyn {
    type Updated = ();

    unsafe fn update(self, data: &mut CellData, slot: Slot, id: Entity, tick: u32) {
        let desc = data.storage.desc();
        unsafe {
            let dst = data.storage.at_mut(slot).unwrap();

            if (self.cmp)(self.value, dst) {
                desc.drop(self.value);
                return;
            }

            desc.drop(dst);

            ptr::copy_nonoverlapping(self.value, dst, desc.size());
        }

        data.set_modified(&[id], Slice::single(slot), tick);
    }
}

impl ComponentPusher for WriteDedupDyn {
    type Pushed = ();

    unsafe fn push(self, data: &mut CellData, id: Entity, tick: u32) {
        let slot = data.storage.len();
        data.storage.extend(self.value, 1);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct ReplaceDyn {
    pub(crate) value: *mut u8,
}

impl ComponentUpdater for ReplaceDyn {
    type Updated = ();

    unsafe fn update(self, data: &mut CellData, slot: Slot, id: Entity, tick: u32) {
        let desc = data.storage.desc();
        unsafe {
            let dst = data.storage.at_mut(slot).unwrap();

            desc.drop(dst);

            ptr::copy_nonoverlapping(self.value, dst, desc.size());
        }

        data.set_modified(&[id], Slice::single(slot), tick);
    }
}

impl ComponentPusher for ReplaceDyn {
    type Pushed = ();

    unsafe fn push(self, data: &mut CellData, id: Entity, tick: u32) {
        let slot = data.storage.len();
        data.storage.extend(self.value, 1);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct MissingDyn {
    pub(crate) value: *mut u8,
}

impl ComponentUpdater for MissingDyn {
    type Updated = ();

    unsafe fn update(self, data: &mut CellData, _: Slot, _: Entity, _: u32) {
        let desc = data.storage.desc();
        unsafe {
            desc.drop(self.value);
        }
    }
}

impl ComponentPusher for MissingDyn {
    type Pushed = ();

    unsafe fn push(self, data: &mut CellData, id: Entity, tick: u32) {
        let slot = data.storage.len();
        data.storage.extend(self.value, 1);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct Buffered<'b> {
    pub(crate) buffer: &'b mut ComponentBuffer,
}

impl<'b> Buffered<'b> {
    pub(crate) fn new(buffer: &'b mut ComponentBuffer) -> Self {
        Self { buffer }
    }
}

unsafe impl<'b> EntityWriter for Buffered<'b> {
    type Output = ();

    fn write(
        self,
        world: &mut World,
        id: Entity,
        src_loc: EntityLocation,
        tick: u32,
    ) -> (EntityLocation, ()) {
        let mut exclusive_relations = Vec::new();

        let arch = world.archetypes.get_mut(src_loc.arch_id);
        unsafe {
            self.buffer.retain(|desc, src| {
                let key = desc.key;
                // The component exists in the current archetype
                // This implies that is it also satisfies any exclusive properties
                if let Some(cell) = arch.cell_mut(key) {
                    let data = cell.data.get_mut();

                    let dst = data.storage.at_mut(src_loc.slot).unwrap();
                    desc.drop(dst);
                    ptr::copy_nonoverlapping(src, dst, desc.size());

                    data.set_modified(&[id], Slice::single(src_loc.slot), tick);
                    false
                } else {
                    // Component does not exist yet, so defer a move

                    // Exclusive relation
                    if key.object.is_some() && desc.meta_ref().has(exclusive()) {
                        if exclusive_relations.contains(&key.id) {
                            panic!("Multiple exclusive relations");
                        }

                        exclusive_relations.push(key.id);
                    }

                    true
                }
            });
        }

        if self.buffer.is_empty() {
            return (src_loc, ());
        }

        // Add the existing components, making sure new exclusive relations are favored
        let (components, _) = find_archetype_components(
            arch.components(),
            self.buffer.components().copied(),
            &exclusive_relations,
        );

        for &desc in self.buffer.components() {
            world.init_component(desc);
        }

        let (dst_id, _) = world.archetypes.find_create(components);

        let (src, dst) = world
            .archetypes
            .get_disjoint(src_loc.arch_id, dst_id)
            .unwrap();

        let (dst_slot, swapped) = unsafe { src.move_to(dst, src_loc.slot, |c, ptr| c.drop(ptr)) };

        // Insert the missing components
        for (desc, src) in self.buffer.drain() {
            unsafe {
                dst.push(desc.key, src, tick);
            }
        }

        let dst_loc = EntityLocation {
            arch_id: dst_id,
            slot: dst_slot,
        };

        update_entity_loc(world, id, dst_loc, swapped);
        // world.archetypes.prune_arch(src_loc.arch_id);

        (dst_loc, ())
    }
}

fn find_archetype_components(
    current_components: impl IntoIterator<Item = ComponentDesc>,
    new_components: impl IntoIterator<Item = ComponentDesc>,
    // Subset of `new_components`
    exclusive: &[Entity],
) -> (Vec<ComponentDesc>, bool) {
    let mut superset = true;
    let res = new_components
        .into_iter()
        .chain(current_components.into_iter().filter(|v| {
            if exclusive.contains(&v.key.id) {
                superset = false;
                false
            } else {
                true
            }
        }))
        .sorted_unstable()
        .collect_vec();

    (res, superset)
}
