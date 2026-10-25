//! The server's filter facets: `Meta.Type[].Filter[]` and a facet's value list.
//!
//! PMS declares, per listing type, which filters it can apply (`genre`, `year`, `resolution`,
//! `hdr`, `unwatched`, ... 28 on a movie library, 15 on shows, 17 on episodes, none on seasons;
//! `docs/library-filters-spec.md`). Each record is `{filter, filterType, key, title, type}`. This
//! module reads the three fields the app uses and nothing else:
//!
//! * `filter` is the facet's NAME. It becomes a URL path segment and a query name, so it is read
//!   only if it is `[A-Za-z0-9_.]{1,48}` ([`valid_facet_name`]): nothing else the server says is
//!   ever interpolated into a request.
//! * `filterType` is read as a [`FacetKind`]: `boolean` is a switch that needs no value list, and
//!   everything else, a type this build has never heard of included, is a list of values.
//! * `title` is the server's label (trimmed, control characters stripped, capped). It is only what
//!   the app shows for a facet whose name it has no word for, and an empty one falls back to the
//!   name, as a sort does.
//!
//! The record's `key` (the server's URL for the value list) is deliberately NOT read. The value
//! path is built from the facet name and the listing's own `type` terms
//! ([`super::library::facet_values_path`]), which reproduces the server's key for every record
//! measured, needs nothing from `Meta` (so it serves a restore that runs before `Meta` lands), and
//! cannot be redirected by a hostile key.
//!
//! A value list's rows are `{key, title}`. The key is the server's, ALREADY percent-encoded
//! (`Not%20Rated`, `5%2E1`, a studio's `X%2520Y`), and is sent back verbatim
//! ([`super::client::QueryBuilder::wire`]).

use serde::Deserialize;

use super::models::{de_str, MediaContainer};

/// Longest facet name read. The server's longest is 16 bytes; a name is a path segment.
pub const MAX_FACET_NAME: usize = 48;
/// Most characters of a server title kept (a facet's or a value's).
pub const MAX_TITLE_CHARS: usize = 64;
/// Longest value key read. A key is a tag id, a short code or a percent-encoded name; a longer one
/// is not worth a request line and is not remembered across a restart either
/// (`session::LibraryView`).
pub const MAX_VALUE_KEY: usize = 96;

/// A facet's shape: a switch that needs no value list, or a list of values to choose from.
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
    /// The server's label; see the module doc for when it is shown.
    pub title: String,
}

/// May `name` be interpolated into a request path and a query name?
pub fn valid_facet_name(name: &str) -> bool {
    (1..=MAX_FACET_NAME).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
}

/// Parameters the listing request carries itself, which a filter term must not be named after.
const RESERVED_NAMES: [&str; 5] = ["type", "sort", "includeMeta", "unwatched", "unwatchedLeaves"];

/// May `name` be a filter TERM, or a value list be asked for? It is a safe [`valid_facet_name`] that is
/// none of the request's own parameters (`type`, `sort`, `includeMeta`) nor the Unwatched switch, which
/// is not a term and whose key answers the whole unwatched LISTING (248 KB on a 400-film library). The
/// server may well advertise `unwatched` as a facet ([`Facet`]); this is about what the app sends.
pub fn valid_term_name(name: &str) -> bool {
    valid_facet_name(name) && !RESERVED_NAMES.contains(&name)
}

/// One `Filter[]` element as the wire has it. Lenient: every field is optional and any scalar
/// shape folds to a string. `key` is not here on purpose (module doc).
#[derive(Deserialize, Default)]
pub struct FilterOption {
    #[serde(default, deserialize_with = "de_str")]
    pub filter: String,
    #[serde(rename = "filterType", default, deserialize_with = "de_str")]
    pub filter_type: String,
    #[serde(default, deserialize_with = "de_str")]
    pub title: String,
}

impl FilterOption {
    /// The facet this record declares, or `None` for a record without a usable name.
    pub fn facet(&self) -> Option<Facet> {
        if !valid_facet_name(&self.filter) {
            return None;
        }
        let kind = if self.filter_type.trim().eq_ignore_ascii_case("boolean") {
            FacetKind::Boolean
        } else {
            FacetKind::List
        };
        let title = clean_title(&self.title);
        let title = if title.is_empty() { self.filter.clone() } else { title };
        Some(Facet { name: self.filter.clone(), kind, title })
    }
}

/// `text` for display: control characters stripped, trimmed, at most [`MAX_TITLE_CHARS`] characters.
pub(super) fn clean_title(text: &str) -> String {
    let stripped: String = text.chars().filter(|c| !c.is_control()).collect();
    stripped.trim().chars().take(MAX_TITLE_CHARS).collect::<String>().trim_end().to_owned()
}

/// One row of a facet's value list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacetValue {
    /// The server's key, verbatim.
    pub key: String,
    pub title: String,
}

/// One window of a facet's value list.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FacetPage {
    pub rows: Vec<FacetValue>,
    /// The list's `totalSize` when the server reported one. A paged answer carries it; an empty
    /// list has no `Directory` and no total, and a window shorter than asked for ends the list on
    /// its own.
    pub total: Option<i64>,
    /// How many rows the server SENT in this window, the skipped ones included: the next window
    /// starts after them, and a short window (fewer than asked for) ends the list, whatever of the
    /// rows could travel as a value.
    pub fetched: usize,
}

impl FacetPage {
    /// The rows of an answer's `Directory[]`. A row with no key, or whose key could not travel as
    /// ONE value (a raw comma splits it, a control character is not a character of any tag, an
    /// over-long one is not worth a request line), is skipped; an empty title falls back to the key.
    pub fn from_container(mc: MediaContainer) -> FacetPage {
        let fetched = mc.directory.len();
        let rows: Vec<FacetValue> = mc
            .directory
            .into_iter()
            .filter(|row| {
                !row.key.is_empty()
                    && row.key.len() <= MAX_VALUE_KEY
                    && !row.key.contains(',')
                    && !row.key.chars().any(char::is_control)
            })
            .map(|row| {
                let title = clean_title(&row.title);
                FacetValue { title: if title.is_empty() { row.key.clone() } else { title }, key: row.key }
            })
            .collect();
        FacetPage { rows, total: (mc.total_size > 0).then_some(mc.total_size), fetched }
    }
}
