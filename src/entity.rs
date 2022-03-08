use core::fmt;
use std::num::NonZeroU64;

#[derive(Clone, Copy, PartialEq)]
pub struct Entity(NonZeroU64);

const ID_MASK: u64 = 0xFFFFFF00;
const GEN_MASK: u64 = !ID_MASK;
impl Entity {
    fn id(&self) -> u64 {
        self.0.get() & ID_MASK
    }

    pub fn generation(&self) -> u64 {
        self.0.get() & GEN_MASK
    }

    pub fn from_bits(bits: NonZeroU64) -> Self {
        Self(bits)
    }

    pub fn to_bits(&self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Entity")
            .field(&self.id())
            .field(&self.generation())
            .finish()
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}:{})", self.id(), self.generation())
    }
}
