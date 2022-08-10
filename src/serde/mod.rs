mod deserialize;

use serde::{de::Visitor, ser::SerializeTupleStruct, Deserialize, Serialize};

use crate::ComponentId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ComponentKey {
    key: String,
    id: ComponentId,
}

impl ComponentKey {
    fn new(key: String, id: ComponentId) -> Self {
        Self { key, id }
    }
}
