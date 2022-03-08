use crate::Entity;

#[derive(Debug, Clone)]
pub struct World {}

impl World {
    pub fn new() -> Self {
        Self {}
    }

    pub fn spawn(&self) -> Entity {
        todo!();
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}
