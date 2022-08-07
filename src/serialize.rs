use std::collections::BTreeMap;

use serde::Serializer;

use crate::{archetype::StorageBorrowDyn, ComponentId};

struct Slot<S>
where
    S: Serializer,
{
    serialize_col: unsafe fn(ser: S, storage: StorageBorrowDyn) -> Result<S::Ok, S::Error>,
    component: ComponentId,
}

pub struct SerializeInfo<S>
where
    S: Serializer,
{
    serializers: BTreeMap<String, Slot<S>>,
}

impl<S> SerializeInfo<S>
where
    S: Serializer,
{
    pub fn new() -> Self {
        todo!()
    }
}
