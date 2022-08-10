use std::collections::BTreeMap;

use erased_serde::Deserializer;
use serde::de::SeqAccess;

use crate::archetype::{ComponentBatch, Storage, StorageBorrowDyn};

use super::ComponentKey;

#[derive(Clone)]
struct Slot {
    /// Takes a whole column and returns a serializer for it
    deser_col: fn(deserializer: &dyn Deserializer, batch: &mut ComponentBatch),
    key: ComponentKey,
}

#[derive(Clone, Default)]
pub struct DeserializeBuilder {
    slots: BTreeMap<String, Slot>,
}

impl DeserializeBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    // pub fn with<T: ComponentValue + serde::Deserialize>(key: impl Into<String>, component: Component<T>) {
    //     fn deser_col(seq: &dyn SeqAccess)
    // }
}
