use core::fmt::Display;

use alloc::vec::Vec;

use crate::{ComponentInfo, Entity};

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
/// The different kinds of errors which can occur
pub enum Error {
    /// The requested entity did not exist
    NoSuchEntity(Entity),
    /// The entity did not have the specified component
    MissingComponent(Entity, ComponentInfo),
    /// A query for a specific entity failed due to an unsatisfied filter
    DoesNotMatch(Entity),
    /// The entity did not match the filter predicate
    Filtered(Entity),
    /// The batch is not complete
    IncompleteBatch,
    /// Attempt to spawn entity with occupied entity id
    EntityOccupied(Entity),
}

impl Error {
    /// Convert the error into an eyre report, regardlees of [std::error::Error] or not.
    pub fn into_eyre(self) -> eyre::Report {
        #[cfg(not(feature = "std"))]
        return eyre::Report::msg(self);

        #[cfg(feature = "std")]
        return eyre::Report::new(self);
    }
}

/// Result alias for [crate::error::Result]
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NoSuchEntity(id) => write!(f, "Entity {id} does not exist"),
            Error::MissingComponent(id, name) => {
                write!(f, "Entity {id} does not have the component {name:?}")
            }
            Error::DoesNotMatch(id) => {
                write!(f, "Entity {id} did not match the query")
            }
            Error::Filtered(id) => {
                write!(f, "Entity {id} did not match the dynamic filter predicate")
            }
            Error::IncompleteBatch => write!(
                f,
                "Attempt to spawn batch with insufficient number of components"
            ),
            Error::EntityOccupied(current) => {
                write!(f, "Attempt to spawn new entity occupied id {current}")
            }
        }
    }
}
