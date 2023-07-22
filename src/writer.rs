use core::{mem, ptr, slice};

use itertools::Itertools;

use crate::{
    archetype::{Cell, Slice, Slot},
    buffer::ComponentBuffer,
    entity::EntityLocation,
    metadata::exclusive,
    world::update_entity_loc,
    ArchetypeId, ComponentInfo, ComponentValue, Entity, World,
};

/// Describes a modification to the components of an entity within the context of an archetype
pub(crate) trait ComponentWriter {
    /// Performs write operations against the target entity
    fn update(self, cell: &mut Cell, slot: Slot, id: Entity, tick: u32);
    /// # Safety
    ///
    /// The cell **must** be extended with valid component data for the new entity
    unsafe fn push(self, cell: &mut Cell, id: Entity, tick: u32);
}

/// # Safety
///
/// The entity must be fully initialized and all bookkepping updated
pub unsafe trait EntityWriter {
    fn write(self, world: &mut World, id: Entity, loc: EntityLocation, tick: u32)
        -> EntityLocation;
}

pub(crate) struct SingleComponentWriter<W> {
    info: ComponentInfo,
    writer: W,
}

impl<W> SingleComponentWriter<W> {
    pub(crate) fn new(info: ComponentInfo, writer: W) -> Self {
        Self { info, writer }
    }
}

unsafe impl<W: ComponentWriter> EntityWriter for SingleComponentWriter<W> {
    fn write(
        self,
        world: &mut World,
        id: Entity,
        src_loc: EntityLocation,
        tick: u32,
    ) -> EntityLocation {
        let key = self.info.key();

        let arch = world.archetypes.get_mut(src_loc.arch_id);

        if let Some(cell) = arch.cell_mut(key) {
            self.writer.update(cell, src_loc.slot, id, tick);
            return src_loc;
        }
        let (src, dst, dst_id) = if let Some(&dst_id) = arch.outgoing.get(&key) {
            eprintln!("Outgoing edge: {:?}", self.info);

            let (src, dst) = world
                .archetypes
                .get_disjoint(src_loc.arch_id, dst_id)
                .unwrap();
            (src, dst, dst_id)
        } else {
            // Oh no! The archetype is missing the component

            eprintln!(
                "Missing component: {:?} found:{:?}",
                self.info,
                arch.components().collect_vec()
            );

            let exclusive = if self.info.meta_ref().has(exclusive()) {
                slice::from_ref(&self.info.key.id)
            } else {
                &[]
            };

            let (components, superset) =
                find_archetype_components(arch.components(), [self.info], exclusive);

            world.init_component(self.info);
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
            } else {
                eprintln!("Not a superset")
            }

            (src, dst, dst_id)
        };

        let (dst_slot, swapped) =
            unsafe { src.move_to(dst, src_loc.slot, |c, ptr| c.drop(ptr), tick) };

        // Insert the missing component
        unsafe {
            let cell = dst
                .cell_mut(key)
                .expect("Missing component in new archetype");

            cell.data.get_mut().storage.reserve(1);
            self.writer.push(cell, id, tick);
        }

        let dst_loc = EntityLocation {
            arch_id: dst_id,
            slot: dst_slot,
        };

        update_entity_loc(world, id, dst_loc, swapped);

        dst_loc
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

pub(crate) struct Replace<'a, T: ComponentValue> {
    pub(crate) value: T,
    pub(crate) output: &'a mut Option<T>,
}

impl<'a, T: ComponentValue> Replace<'a, T> {
    pub(crate) fn new(value: T, output: &'a mut Option<T>) -> Self {
        Self { value, output }
    }
}

impl<'a, T: ComponentValue> ComponentWriter for Replace<'a, T> {
    fn update(self, cell: &mut Cell, slot: Slot, id: Entity, tick: u32) {
        let data = cell.data.get_mut();

        let storage = data.storage.downcast_mut::<T>();
        let old = mem::replace(&mut storage[slot], self.value);

        data.set_modified(&[id], Slice::single(slot), tick);

        *self.output = Some(old);
    }

    unsafe fn push(mut self, cell: &mut Cell, id: Entity, tick: u32) {
        let data = cell.data.get_mut();

        let slot = data.storage.len();

        data.storage.extend(&mut self.value as *mut T as *mut u8, 1);

        mem::forget(self.value);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct ReplaceDyn {
    pub(crate) value: *mut u8,
}

impl ComponentWriter for ReplaceDyn {
    fn update(self, cell: &mut Cell, slot: Slot, id: Entity, tick: u32) {
        let info = cell.info();

        let data = cell.data.get_mut();

        unsafe {
            let dst = data.storage.at_mut(slot).unwrap();

            info.drop(dst);

            ptr::copy_nonoverlapping(self.value, dst, info.size());
        }

        data.set_modified(&[id], Slice::single(slot), tick);
    }

    unsafe fn push(self, cell: &mut Cell, id: Entity, tick: u32) {
        let data = cell.data.get_mut();

        let slot = data.storage.len();
        data.storage.extend(self.value, 1);

        data.set_added(&[id], Slice::single(slot), tick);
    }
}

pub(crate) struct Buffered<'a> {
    pub(crate) buffer: &'a mut ComponentBuffer,
}

impl<'a> Buffered<'a> {
    pub(crate) fn new(buffer: &'a mut ComponentBuffer) -> Self {
        Self { buffer }
    }
}

unsafe impl<'a> EntityWriter for Buffered<'a> {
    fn write(
        self,
        world: &mut World,
        id: Entity,
        src_loc: EntityLocation,
        tick: u32,
    ) -> EntityLocation {
        let mut exclusive_relations = Vec::new();

        let arch = world.archetypes.get_mut(src_loc.arch_id);
        unsafe {
            self.buffer.retain(|info, src| {
                let key = info.key;
                // The component exists in the current archetype
                // This implies that is it also satisfies any exclusive properties
                if let Some(cell) = arch.cell_mut(key) {
                    let data = cell.data.get_mut();

                    let dst = data.storage.at_mut(src_loc.slot).unwrap();
                    info.drop(dst);
                    ptr::copy_nonoverlapping(src, dst, info.size());

                    data.set_modified(&[id], Slice::single(src_loc.slot), tick);
                    false
                } else {
                    // Component does not exist yet, so defer a move

                    // Exclusive relation
                    if key.object.is_some() && info.meta_ref().has(exclusive()) {
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
            eprintln!("Archetype fully matched");
            return src_loc;
        }

        // Add the existing components, making sure new exclusive relations are favored
        let (components, _) = find_archetype_components(
            arch.components(),
            self.buffer.components().copied(),
            &exclusive_relations,
        );

        for &info in self.buffer.components() {
            eprintln!("Initializing component {:?}", info);
            world.init_component(info);
        }

        let (dst_id, _) = world.archetypes.find_create(components);

        let (src, dst) = world
            .archetypes
            .get_disjoint(src_loc.arch_id, dst_id)
            .unwrap();

        let (dst_slot, swapped) =
            unsafe { src.move_to(dst, src_loc.slot, |c, ptr| c.drop(ptr), tick) };

        // Insert the missing components
        for (info, src) in self.buffer.drain() {
            unsafe {
                dst.push(info.key, src, tick);
            }
        }

        let dst_loc = EntityLocation {
            arch_id: dst_id,
            slot: dst_slot,
        };

        update_entity_loc(world, id, dst_loc, swapped);
        world.archetypes.prune_arch(src_loc.arch_id);

        dst_loc
    }
}

fn find_archetype_components(
    current_components: impl IntoIterator<Item = ComponentInfo>,
    new_components: impl IntoIterator<Item = ComponentInfo>,
    // Subset of `new_components`
    exclusive: &[Entity],
) -> (Vec<ComponentInfo>, bool) {
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

    dbg!(res, superset)
}
