mod deserialize;
mod serialize;

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

#[derive(serde::Serialize, serde::Deserialize)]
enum WorldFields {
    #[serde(rename = "archetypes")]
    Archetypes,
}
