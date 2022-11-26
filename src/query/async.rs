use crate::{notify::Notify, Query};

/// A query which allows awaiting for changes
pub struct AsyncQuery<Q, F> {
    query: Query<Q, F>,
    notify: Notify,
}
