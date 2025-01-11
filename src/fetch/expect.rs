use alloc::borrow::Cow;

use super::{Fetch, FetchItem, FmtQuery};

/// Expect the query to match, panic otherwise
pub struct Expect<Q> {
    msg: Option<Cow<'static, str>>,
    fetch: Q,
}

impl<Q> Expect<Q> {
    /// Expect the query to match, panic otherwise
    pub fn new(fetch: Q, msg: Option<Cow<'static, str>>) -> Self {
        Self { fetch, msg }
    }
}

impl<'q, Q: FetchItem<'q>> FetchItem<'q> for Expect<Q> {
    type Item = Q::Item;
}

impl<'w, Q: Fetch<'w>> Fetch<'w> for Expect<Q> {
    const MUTABLE: bool = Q::MUTABLE;

    type Prepared = Q::Prepared;

    fn prepare(&'w self, data: super::FetchPrepareData<'w>) -> Option<Self::Prepared> {
        match self.fetch.prepare(data) {
            Some(v) => Some(v),
            None => match &self.msg {
                Some(msg) => panic!("{msg}"),
                None => panic!(
                    "Expected {:?} to be present on entity",
                    FmtQuery(&self.fetch)
                ),
            },
        }
    }

    fn filter_arch(&self, _: super::FetchAccessData) -> bool {
        true
    }

    fn access(
        &self,
        data: super::FetchAccessData,
        dst: &mut alloc::vec::Vec<crate::system::Access>,
    ) {
        self.fetch.access(data, dst)
    }

    fn describe(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("expect ")?;
        self.fetch.describe(f)
    }
}
