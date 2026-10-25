//! The server's filter facets (`Meta.Type[].Filter[]`) and the reads that go with them.
//!
//! STUB: the red commit of PR 1a names the types and signatures the tests need, with neutral
//! bodies. The next commit replaces them.

use super::models::MediaContainer;

/// A facet's shape: a switch that needs no value list, or a list of values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacetKind {
    Boolean,
    List,
}

/// One filter the server offers for a listing type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facet {
    pub name: String,
    pub kind: FacetKind,
    pub title: String,
}

/// One row of a facet's value list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacetValue {
    pub key: String,
    pub title: String,
}

/// One window of a facet's value list.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FacetPage {
    pub rows: Vec<FacetValue>,
    pub total: Option<i64>,
}

impl FacetPage {
    pub fn from_container(_mc: MediaContainer) -> FacetPage {
        FacetPage::default()
    }
}

/// May `name` be interpolated into a request path and a query name?
pub fn valid_facet_name(_name: &str) -> bool {
    true
}
