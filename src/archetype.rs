use std::ptr::NonNull;

#[derive(Debug, Clone, PartialEq)]
pub struct Archetype {
    components: Box<Data>,
}

#[derive(Debug, Clone, PartialEq)]
struct Data {
    components: NonNull<u8>,
}
