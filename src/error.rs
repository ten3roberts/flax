use thiserror::Error;

use crate::{ComponentInfo, Entity, EntityKind};

#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    #[error("No entities exist for {0:?}")]
    NoSuchKind(EntityKind),
    #[error("The entity {0} does not exist or has been despawned.")]
    NoSuchEntity(Entity),
    #[error("The entity {0} does not have the component {1:?}.")]
    MissingComponent(Entity, &'static str),
    #[error("The entity {0} did not match the fetch {1:?}.\nMissing {2:?}.")]
    UnmatchedFetch(Entity, String, Vec<String>),
    #[error("{0} is already borrowed mutably")]
    Borrow(&'static str),
    #[error("{0} can not be borrowed mutably as it is already borrowed")]
    BorrowMut(&'static str),
    #[error("Entities {0:?} were not disjoint")]
    Disjoint(Vec<Entity>),
    #[error("Could not downcast cell to concrete type {0}")]
    Downcast(&'static str),

    #[error("Entity {0} already exists")]
    EntityExists(Entity),
    #[error("The component has already been added in the batch")]
    DuplicateComponent(ComponentInfo),
    #[error("Attempt to spawn batch with an insufficient number of components")]
    IncompleteBatch,
}

#[derive(Debug, Error)]
#[error("Failed to execute system {:?}", name.as_ref().map(|v| v.as_str()).unwrap_or_else(|| "unnkown"))]
pub struct SystemError {
    pub(crate) name: Option<String>,
    #[source]
    pub(crate) report: eyre::Report,
}

pub type Result<T> = std::result::Result<T, Error>;
pub type SystemResult<T> = std::result::Result<T, SystemError>;
