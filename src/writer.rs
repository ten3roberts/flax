use core::mem;

use itertools::{Either, Itertools};

use crate::{
    archetype::{Archetype, Slot},
    entity::EntityLocation,
    ArchetypeId, Component, ComponentInfo, ComponentValue, Entity, World,
};

pub(crate) trait ComponentUpdater {
    /// If returned, will be used to migrate the entity to a new archetype
    type Writer: ComponentWriter;

    fn update(
        self,
        archetype: &mut Archetype,
        id: Entity,
        slot: Slot,
        tick: u32,
    ) -> Option<Self::Writer>;
}

pub(crate) trait ComponentWriter {
    fn migrate(
        self,
        world: &mut World,
        src_id: ArchetypeId,
        src_slot: Slot,
        tick: u32,
    ) -> (EntityLocation, Option<(Entity, Slot)>);
}

struct Replace<T: ComponentValue> {
    component: Component<T>,
    value: T,
}

impl<T: ComponentValue> ComponentUpdater for Replace<T> {
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

struct ReplaceWriter<T> {
    dst: Either<ArchetypeId, Vec<ComponentInfo>>,
    component: Component<T>,
    value: T,
}

impl<T: ComponentValue> ComponentWriter for ReplaceWriter<T> {
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
