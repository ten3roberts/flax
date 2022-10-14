use core::fmt::Display;

use alloc::{string::String, vec::Vec};

use crate::{ComponentInfo, Entity};

// /// Additional (optional) context as to why a query failed
// #[derive(Debug, PartialEq, Eq)]
// pub struct MismatchedQuery {
//     id: Entity,
//     query: String,
//     reason: MismatchedQueryReason,
// }

// #[derive(PartialEq, Eq, Debug, Clone)]
// /// Describes what caused a query to fail
// #[non_exhaustive]
// pub enum MismatchedQueryReason {
//     /// The filter did not match
//     Filter,
//     /// The fetch did not match
//     Fetch {
//         /// The missing components
//         missing: Vec<ComponentInfo>,
//     },
// }

// impl Display for MismatchedQuery {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         let Self { id, query, reason } = self;
//         write!(f, "Entity {id} did not match the query {query:?}. {reason}")
//     }
// }

// impl Display for MismatchedQueryReason {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         match self {
//             MismatchedQueryReason::Filter => write!(f, "The entity did not match the filter"),
//             MismatchedQueryReason::Fetch { missing } => {
//                 write!(
//                     f,
//                     "The entity did not match the fetch. Missing: {missing:?}"
//                 )
//             }
//         }
//     }
// }

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
/// The different kinds of errors which can occur
pub enum Error {
    /// The requested entity did not exist
    NoSuchEntity(Entity),
    /// The entity did not have the specified component
    MissingComponent(Entity, ComponentInfo),
    /// A query for a specific entity failed due to an unsatisfied filter
    MismatchedFilter(Entity),
    /// Attempt to access the same entity mutably
    Disjoint(Vec<Entity>),
    /// The batch is not complete
    IncompleteBatch,
    /// Attempt to spawn entity with occupied entity id
    EntityOccupied(Entity),
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
            Error::MismatchedFilter(id) => {
                write!(f, "Entity {id} did not match the query filter")
            }
            Error::Disjoint(ids) => write!(f, "Entities {ids:?} were not disjoint"),
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
