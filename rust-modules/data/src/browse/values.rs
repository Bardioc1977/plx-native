//! A facet's value list as the section holds it: the rows that have landed, in the server's order.
//!
//! STUB: the red commit of the value-list job names the type the tests need. The next commit
//! replaces it.

use super::filters::FilterValue;

/// The rows of one facet's value list that have landed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValueList {
    pub rows: Vec<FilterValue>,
    pub total: Option<i64>,
    pub complete: bool,
    pub generation: u32,
    pub revision: u32,
    pub failed: Option<u32>,
}
