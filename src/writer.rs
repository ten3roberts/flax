use core::{mem, ptr};

use itertools::{Either, Itertools};

use crate::{
    archetype::{Archetype, Slice, Slot},
    buffer::ComponentBuffer,
    entity::EntityLocation,
    metadata::exclusive,
    ArchetypeId, Component, ComponentInfo, ComponentValue, Entity, World,
};

pub(crate) trait ComponentUpdater {
    /// If returned, will be used to migrate the entity to a new archetype
    type Writer: MigrateEntity;

    fn update(
        self,
        archetype: &mut Archetype,
        id: Entity,
        slot: Slot,
        tick: u32,
    ) -> Option<Self::Writer>;
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
    pub(crate) component: Component<T>,
    pub(crate) value: T,
    pub(crate) output: &'a mut Option<T>,
}

impl<'a, T: ComponentValue> ComponentUpdater for Replace<'a, T> {
    type Writer = ReplaceWriter<T>;

    fn update(
        self,
        arch: &mut Archetype,
        id: Entity,
        slot: Slot,
        tick: u32,
    ) -> Option<Self::Writer> {
        let key = self.component.key();

        if let Some(cell) = arch.cell_mut(key) {
            let data = cell.data.get_mut();

            let storage = data.storage.downcast_mut::<T>();
            let old = mem::replace(&mut storage[slot], self.value);

            data.set_modified(&[id], Slice::single(slot), tick);

            *self.output = Some(old);

            None
        } else if let Some(&dst) = arch.outgoing.get(&key) {
            eprintln!("Outgoing edge: {:?}", self.component);

            Some(ReplaceWriter {
                dst: Either::Left(dst),
                component: self.component,
                value: self.value,
            })
        } else {
            // Oh no! The archetype is missing the component
            //
            // Generate a list of component infos which fully satisfy the requirements for the
            // desired archetype to move to
            let pivot = arch.components().take_while(|v| v.key < key).count();

            // Split the components
            // A B C [new] D E F
            let left = arch.components().take(pivot);
            let right = arch.components().skip(pivot);

            let components = left
                .chain([self.component.info()])
                .chain(right)
                .collect_vec();

            Some(ReplaceWriter {
                dst: Either::Right(components),
                component: self.component,
                value: self.value,
            })
        }
    }
}

pub(crate) struct ReplaceWriter<T> {
    dst: Either<ArchetypeId, Vec<ComponentInfo>>,
    component: Component<T>,
    value: T,
}

unsafe impl<T: ComponentValue> MigrateEntity for ReplaceWriter<T> {
    fn migrate(
        self,
        world: &mut World,
        src_id: ArchetypeId,
        src_slot: Slot,
        tick: u32,
    ) -> (EntityLocation, Option<(Entity, Slot)>) {
        let key = self.component.key();

        let (src, dst, dst_id) = match &self.dst {
            &Either::Left(dst_id) => {
                let (src, dst) = world.archetypes.get_disjoint(src_id, dst_id).unwrap();
                (src, dst, dst_id)
            }
            Either::Right(components) => {
                // Initialize component
                world.init_component(self.component.info());

                let (dst_id, _) = world.archetypes.find(components.iter().copied());

                // Add a quick edge to refer to later
                let (src, dst) = world.archetypes.get_disjoint(src_id, dst_id).unwrap();
                src.add_outgoing(key, dst_id);
                dst.add_incoming(key, src_id);
                (src, dst, dst_id)
            }
        };

        let (dst_slot, swapped) = unsafe { src.move_to(dst, src_slot, |c, ptr| c.drop(ptr), tick) };

        // Insert the missing component
        unsafe {
            let mut value = self.value;
            dst.push(key, &mut value as *mut T as *mut u8, tick);
            mem::forget(value);
        }

        (
            EntityLocation {
                slot: dst_slot,
                arch_id: dst_id,
            },
            swapped,
        )
    }
}

pub(crate) struct Buffered<'a> {
    pub(crate) buffer: &'a mut ComponentBuffer,
}

impl<'a> ComponentUpdater for Buffered<'a> {
    type Writer = BufferedMigrate<'a>;

    fn update(
        self,
        arch: &mut Archetype,
        id: Entity,
        slot: Slot,
        tick: u32,
    ) -> Option<Self::Writer> {
        let mut exclusive_relations = Vec::new();
        unsafe {
            self.buffer.retain(|info, src| {
                let key = info.key;
                if let Some(cell) = arch.cell_mut(key) {
                    let data = cell.data.get_mut();

                    let dst = data.storage.at_mut(slot).unwrap();
                    info.drop(dst);
                    ptr::copy_nonoverlapping(dst as *mut (), src, info.size());

                    data.set_modified(&[id], Slice::single(slot), tick);
                    false
                } else {
                    // Component does not exist yet, so defer a move

                    // Exclusive relation
                    if key.object.is_some()
                        && info.meta_ref().has(exclusive())
                        && !exclusive_relations.contains(&key.id)
                    {
                        exclusive_relations.push(key.id);
                    }

                    true
                }
            });
        }

        if self.buffer.is_empty() {
            eprintln!("Archetype fully matched");
            None
        } else {
            // Add the existing components, making sure new exclusive relations are favored
            let components = self
                .buffer
                .components()
                .copied()
                .chain(
                    arch.components()
                        .filter(|v| !exclusive_relations.contains(&v.key.id)),
                )
                .sorted_unstable()
                .collect_vec();

            Some(BufferedMigrate {
                components,
                buffer: self.buffer,
            })
        }
    }
}

pub(crate) struct BufferedMigrate<'a> {
    components: Vec<ComponentInfo>,
    buffer: &'a mut ComponentBuffer,
}

unsafe impl<'a> MigrateEntity for BufferedMigrate<'a> {
    fn migrate(
        self,
        world: &mut World,
        src_id: ArchetypeId,
        src_slot: Slot,
        tick: u32,
    ) -> (EntityLocation, Option<(Entity, Slot)>) {
        for &info in self.buffer.components() {
            eprintln!("Initializing component {:?}", info);
            world.init_component(info);
        }

        let (dst_id, _) = world.archetypes.find(self.components.iter().copied());

        let (src, dst) = world.archetypes.get_disjoint(src_id, dst_id).unwrap();
        let (dst_slot, swapped) = unsafe { src.move_to(dst, src_slot, |c, ptr| c.drop(ptr), tick) };

        // Insert the missing components
        for (info, src) in self.buffer.drain() {
            unsafe {
                // src moves into herer
                dst.push(info.key, src, tick);
            }
        }

        eprintln!("Buffer retained {} items", self.buffer.len());

        (
            EntityLocation {
                slot: dst_slot,
                arch_id: dst_id,
            },
            swapped,
        )
    }
}
