use thiserror::Error;

use crate::Entity;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
/// The different kind of errors which can occur
pub enum Error {
    /// The requested entity did not exist
    #[error("Entity {0} does not exist or has been despawned.")]
    NoSuchEntity(Entity),
    /// The entity did not have the specified component
    #[error("Entity {0} does not have the component {1:?}.")]
    MissingComponent(Entity, &'static str),
    /// The fetch_one failed due to missing components
    #[error("Entity {0} did not match the fetch {1:?}.\nMissing {2:?}.")]
    UnmatchedFetch(Entity, String, Vec<String>),
    /// Attempt to access the same entity mutably
    #[error("Entities {0:?} were not disjoint")]
    Disjoint(Vec<Entity>),
    /// The batch is not complete
    #[error("Attempt to spawn batch with an insufficient number of components")]
    IncompleteBatch,
}

/// Result alias for [crate::error::Result]
pub type Result<T> = std::result::Result<T, Error>;
