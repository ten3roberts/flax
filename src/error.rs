use core::fmt::Display;

use crate::{ComponentDesc, Entity};

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
/// The different kinds of errors which can occur
pub enum Error {
    /// The requested entity did not exist
    NoSuchEntity(Entity),
    /// The entity did not have the specified component
    MissingComponent(MissingComponent),
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
    /// Convert the error into an anyhow report, regardles of [std::error::Error] or not.
    pub(crate) fn into_anyhow(self) -> anyhow::Error {
        #[cfg(not(feature = "std"))]
        return anyhow::Error::msg(self);

        #[cfg(feature = "std")]
        return anyhow::Error::new(self);
    }

    pub(crate) fn try_into_missing_component(self) -> std::result::Result<MissingComponent, Self> {
        if let Self::MissingComponent(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}

impl From<MissingComponent> for Error {
    fn from(value: MissingComponent) -> Self {
        Self::MissingComponent(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Missing component
pub struct MissingComponent {
    /// The entity which did not have the component
    pub id: Entity,
    /// The missing component
    pub desc: ComponentDesc,
}

/// Result alias for [crate::error::Result]
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NoSuchEntity(id) => write!(f, "Entity {id} does not exist"),
            Error::MissingComponent(inner) => Display::fmt(inner, f),
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

impl Display for MissingComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Entity {} does not have the component {:?}",
            self.id, self.desc
        )
    }
}
