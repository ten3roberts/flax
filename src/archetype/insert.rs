use core::slice;
use std::{
    collections::{btree_map::Entry, BTreeMap},
    mem::MaybeUninit,
};

use crate::{Component, ComponentId, ComponentValue, Error};

use super::Storage;

pub struct BatchSpawn {
    len: usize,
    storage: BTreeMap<ComponentId, Storage>,
}

impl BatchSpawn {
    pub fn new(len: usize) -> Self {
        Self {
            len,
            storage: Default::default(),
        }
    }

    /// Insert values for a component. The number of the items in the iterator
    /// must match the `len` given to the `BatchSpawn`
    pub fn insert<T: ComponentValue>(
        &mut self,
        component: Component<T>,
        iter: impl IntoIterator<Item = T>,
    ) -> crate::error::Result<()> {
        let info = component.info();
        match self.storage.entry(component.id()) {
            Entry::Occupied(_) => Err(Error::DuplicateComponent(info)),
            Entry::Vacant(slot) => {
                let storage = slot.insert(Storage::new(self.len, info));

                let ptr = storage.as_ptr().cast::<T>();
                let stride = storage.info().size();

                let mut count = 0;

                for item in iter.into_iter().take(self.len) {
                    unsafe {
                        let base = ptr.add(count * stride);
                        base.write(item)
                    }

                    count += 1;
                }

                if count != self.len {
                    // Drop what we have
                    for slot in 0..count {
                        unsafe { (info.drop)(storage.at_mut(slot)) }
                    }

                    Err(Error::IncompleteBatch)
                } else {
                    Ok(())
                }
            }
        }
    }
}
