use std::{
    collections::{btree_map::Entry, BTreeMap},
    mem,
};

use crate::{Component, ComponentId, ComponentInfo, ComponentValue, Error};

use super::Storage;

pub struct ComponentBatch {
    len: usize,
    storage: BTreeMap<ComponentId, Storage>,
}

impl ComponentBatch {
    pub fn new(len: usize) -> Self {
        Self {
            len,
            storage: Default::default(),
        }
    }

    pub fn components(&self) -> impl Iterator<Item = &ComponentInfo> {
        self.storage.values().map(|v| v.info())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Set values for a specific component. The number of the items in the iterator
    /// must match the `len` given to the `BatchSpawn`
    pub fn set<T: ComponentValue>(
        &mut self,
        component: Component<T>,
        iter: impl IntoIterator<Item = T>,
    ) -> crate::error::Result<()> {
        let info = component.info();
        match self.storage.entry(component.id()) {
            Entry::Occupied(_) => Err(Error::DuplicateComponent(info)),
            Entry::Vacant(slot) => {
                let storage = slot.insert(Storage::with_capacity(info, self.len));

                for mut item in iter.into_iter() {
                    unsafe {
                        storage.extend(&mut item as *mut _ as *mut u8, 1);
                        mem::forget(item);
                    }
                }

                // let iter = iter.into_iter();

                // let ptr = storage.as_ptr().cast::<T>();
                // let stride = storage.info().size();

                // let mut count = 0;

                // for item in iter.into_iter().take(self.len) {
                //     unsafe {
                //         let base = ptr.add(count * stride);
                //         base.write(item)
                //     }

                //     count += 1;
                // }

                if storage.len() != self.len {
                    Err(Error::IncompleteBatch)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(crate) fn take_all(mut self) -> impl Iterator<Item = (ComponentId, Storage)> {
        mem::take(&mut self.storage).into_iter()
    }
}
