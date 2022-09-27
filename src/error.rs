use core::fmt::Display;

use alloc::{string::String, vec::Vec};

use crate::Entity;

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
/// The different kind of errors which can occur
pub enum Error {
    /// The requested entity did not exist
    NoSuchEntity(Entity),
    /// The entity did not have the specified component
    MissingComponent(Entity, &'static str),
    /// The fetch_one failed due to missing components
    UnmatchedFetch(Entity, String, Vec<String>),
    /// Attempt to access the same entity mutably
    Disjoint(Vec<Entity>),
    /// The batch is not complete
    IncompleteBatch,
}

impl Error {
    #[cfg(feature = "std")]
    pub(crate) fn into_eyre(self) -> eyre::Report {
        eyre::Report::new(self)
    }

    #[cfg(not(feature = "std"))]
    pub(crate) fn into_eyre(self) -> eyre::Report {
        eyre::Report::msg(self)
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
            Error::UnmatchedFetch(id, fetch, missing) => write!(
                f,
                "Entity {id} did not match the fetch {fetch:?}.\nMissing: {missing:?}"
            ),
            Error::Disjoint(ids) => write!(f, "Entities {ids:?} were not disjoint"),
            Error::IncompleteBatch => write!(
                f,
                "Attempt to spawn batch with insufficient number of components"
            ),
        }
    }
}

// #[derive(Debug)]
// /// Commandbuffer failed to apply.
// /// Each fallible variant of [`crate::commands::Command`] has a corresponding error variant
// enum ApplyError {
//     Set { inner: Error, name: &'static str },
//     Despawn(Error),
//     Remove { inner: Error, name: &'static str },
//     Defer(eyre::Result<()>),
// }

// impl Display for ApplyError {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         match self {
//             ApplyError::Set { name, .. } => write!(f, "Failed to set component {:?}", name),
//             ApplyError::Despawn(_) => write!(f, "Failed to despawn entity"),
//             ApplyError::Remove { name, .. } => write!(f, "Failed to remove component {name}"),
//             ApplyError::Defer(_) => todo!(),
//         }
//     }
// }
