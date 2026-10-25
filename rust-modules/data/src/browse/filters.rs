//! The Library's filter terms: which facets the viewer narrowed the listing by, and to what.
//!
//! STUB: the red commit of PR 1b names the types and signatures the tests need, with neutral
//! bodies. The next commit replaces them.

use super::{LibraryType, SecKind};

/// One value of a facet: the server's key (verbatim, already percent-encoded) and its title.
#[derive(Clone, Debug)]
pub struct FilterValue {
    pub id: String,
    pub title: String,
}

impl PartialEq for FilterValue {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.title == other.title
    }
}
impl Eq for FilterValue {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterTerm {
    pub facet: String,
    pub flag: bool,
    pub values: Vec<FilterValue>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterSet {
    terms: Vec<FilterTerm>,
}

pub const MAX_QUERY_BYTES: usize = 4096;
pub const VALUE_PAGE: usize = 100;
pub const VALUE_LISTS_KEPT: usize = 3;
pub const RESTORE_VOUCH_PAGE: usize = 500;

impl FilterSet {
    pub fn from_terms(terms: impl IntoIterator<Item = FilterTerm>) -> Self {
        Self { terms: terms.into_iter().collect() }
    }
    pub fn terms(&self) -> &[FilterTerm] {
        &self.terms
    }
    pub fn get(&self, _facet: &str) -> Option<&FilterTerm> {
        None
    }
    pub fn len(&self) -> usize {
        self.terms.len()
    }
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
    pub fn with(&self, _facet: &str, _values: Vec<FilterValue>) -> FilterSet {
        self.clone()
    }
    pub fn with_flag(&self, _facet: &str, _on: bool) -> FilterSet {
        self.clone()
    }
    pub fn without(&self, _facet: &str) -> FilterSet {
        self.clone()
    }
    pub fn cleared(&self) -> FilterSet {
        self.clone()
    }
    pub fn wire(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    pub fn wire_bytes(&self) -> usize {
        0
    }
    pub fn retain_facets(&self, _keep: impl Fn(&str) -> bool) -> (FilterSet, Vec<String>) {
        (self.clone(), Vec::new())
    }
}

/// The wire name of the Unwatched switch for a listing.
pub fn unwatched_facet(_kind: SecKind, _listing: LibraryType) -> &'static str {
    "unwatched"
}

/// The terms a value-list read carries.
pub fn value_type_terms(_listing: LibraryType, _kind: SecKind) -> Vec<(String, String)> {
    Vec::new()
}
