//! The Library's filter terms: which facets the viewer narrowed the listing by, and to what.
//!
//! The server declares, per listing type, the filters it can apply (`plx_plex::plex::Facet`,
//! `docs/library-filters-spec.md`). A selection is a [`FilterSet`]: at most one [`FilterTerm`] per
//! facet, AND across facets and OR within one. On the wire a term is ONE parameter named after the
//! facet, its value keys comma-joined (`genre=12,7&resolution=4k&hdr=1`), and `SecState::query_filters`
//! is the one place that writes them, for the grid's page, the A-Z letters read and the Collections loan.
//!
//! **A value is its key.** [`FilterValue`] has the shape of the genre entry it generalizes (`id` is the
//! server's key, verbatim and already percent-encoded; `title` is display and refreshed when a list
//! lands), and two values, two terms and two sets are equal when their keys are: a renamed value is
//! not a different query, and the no-op test of an edit and the letters' key depend on it.
//!
//! **The Unwatched switch is not a term.** It stays one flag of the section that carries across the
//! listing types (a show's Unwatched is `unwatchedLeaves`, an episode's `unwatched`;
//! [`unwatched_facet`] names it), needs no `Meta`, and is never reconciled against it. Every other
//! term belongs to one listing type.

use super::{LibraryType, SecKind};

/// One value of a facet: the server's key and its title.
#[derive(Clone, Debug)]
pub struct FilterValue {
    /// The key the server listed it under, sent back verbatim (`plx_plex::plex::QueryBuilder::wire`).
    pub id: String,
    pub title: String,
}

impl PartialEq for FilterValue {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for FilterValue {}

/// The chosen values of one facet, or a boolean facet switched on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterTerm {
    pub facet: String,
    /// A boolean facet that is on (`hdr=1`); its `values` are empty.
    pub flag: bool,
    /// Sorted by key, no repeats.
    pub values: Vec<FilterValue>,
}

/// The selection: canonical, so equal selections are equal values. Terms are sorted by facet name,
/// there is one per facet (the first wins), values are sorted by key and deduped, and a term with
/// nothing in it is no term.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterSet {
    terms: Vec<FilterTerm>,
}

/// What the filter terms of one request may take of the request line, in wire bytes. The PMS
/// answers 200 up to at least 8 KB of filter values and 500 at 16 KB (measured, direct, 2026-10-25);
/// relays and proxies may be stricter, so the app stays at half of the lowest measured success. An
/// edit that would pass it is refused; this bounds a request, not what the viewer can choose.
pub const MAX_QUERY_BYTES: usize = 4096;
/// Rows of a facet's value list asked for at a time.
pub const VALUE_PAGE: usize = 100;
/// Value lists a section keeps, most recently opened first.
pub const VALUE_LISTS_KEPT: usize = 3;
/// The page a restore reads the genre list in, so a library's whole genre list is vouched as before.
pub const RESTORE_VOUCH_PAGE: usize = 500;

impl FilterSet {
    /// The canonical set of `terms`.
    pub fn from_terms(terms: impl IntoIterator<Item = FilterTerm>) -> Self {
        let mut kept: Vec<FilterTerm> = Vec::new();
        for mut term in terms {
            if kept.iter().any(|seen| seen.facet == term.facet) {
                continue;
            }
            if term.flag {
                term.values.clear();
            } else {
                term.values.sort_by(|a, b| a.id.cmp(&b.id));
                term.values.dedup_by(|a, b| a.id == b.id);
                if term.values.is_empty() {
                    continue;
                }
            }
            kept.push(term);
        }
        kept.sort_by(|a, b| a.facet.cmp(&b.facet));
        Self { terms: kept }
    }

    pub fn terms(&self) -> &[FilterTerm] {
        &self.terms
    }

    pub fn get(&self, facet: &str) -> Option<&FilterTerm> {
        self.terms.iter().find(|term| term.facet == facet)
    }

    /// How many facets are narrowed: the N of "Filters . N".
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// This set with `facet` narrowed to `values` (none: the facet is cleared).
    pub fn with(&self, facet: &str, values: Vec<FilterValue>) -> FilterSet {
        let mut terms: Vec<FilterTerm> = self.terms.iter().filter(|term| term.facet != facet).cloned().collect();
        terms.push(FilterTerm { facet: facet.to_owned(), flag: false, values });
        FilterSet::from_terms(terms)
    }

    /// This set with the boolean `facet` switched `on` or off.
    pub fn with_flag(&self, facet: &str, on: bool) -> FilterSet {
        let mut terms: Vec<FilterTerm> = self.terms.iter().filter(|term| term.facet != facet).cloned().collect();
        if on {
            terms.push(FilterTerm { facet: facet.to_owned(), flag: true, values: Vec::new() });
        }
        FilterSet::from_terms(terms)
    }

    pub fn without(&self, facet: &str) -> FilterSet {
        FilterSet::from_terms(self.terms.iter().filter(|term| term.facet != facet).cloned())
    }

    /// The empty selection.
    pub fn cleared(&self) -> FilterSet {
        FilterSet::default()
    }

    /// The terms as wire pairs `(facet, keys)`, in canonical order: the keys comma-joined, a boolean
    /// `1`. `SecState::query_filters` appends them after the listing's `type=` term and the Unwatched
    /// switch.
    pub fn wire(&self) -> Vec<(String, String)> {
        self.terms
            .iter()
            .map(|term| {
                let keys = if term.flag {
                    "1".to_owned()
                } else {
                    term.values.iter().map(|value| value.id.as_str()).collect::<Vec<_>>().join(",")
                };
                (term.facet.clone(), keys)
            })
            .collect()
    }

    /// What the terms cost of the request line, in wire bytes (`&name=value` for each).
    pub fn wire_bytes(&self) -> usize {
        self.wire().iter().map(|(name, keys)| 2 + name.len() + plx_plex::plex::wire_len(keys)).sum()
    }

    /// The terms whose facet `keep` accepts, and the names of the facets dropped.
    pub fn retain_facets(&self, keep: impl Fn(&str) -> bool) -> (FilterSet, Vec<String>) {
        let dropped = self.terms.iter().filter(|term| !keep(&term.facet)).map(|term| term.facet.clone()).collect();
        (FilterSet::from_terms(self.terms.iter().filter(|term| keep(&term.facet)).cloned()), dropped)
    }
}

/// The wire name of the Unwatched switch for `listing` of a section of `kind`. A show is unwatched
/// by its leaves (`unwatchedLeaves`, which a season also answers); an episode and a movie by
/// themselves. The one place the mapping is written.
pub fn unwatched_facet(kind: SecKind, listing: LibraryType) -> &'static str {
    match (kind, listing) {
        (SecKind::Show, LibraryType::Primary | LibraryType::Seasons) => "unwatchedLeaves",
        _ => "unwatched",
    }
}

/// The terms a facet's value-list read carries, for the listing that is shown: its own `type=`
/// term, and nothing else. ONE rule for the picker and the restore, which needs it before `Meta`
/// has landed. It reproduces the server's own `key` of every facet measured, because the `?type=N`
/// the server attaches always equals the listing type and, where it attaches none, the list is the
/// same with and without it; the one place the type matters (`year`, a show's `director`) is
/// exactly where the server's key carries it. No filter term is sent, so the list is the whole,
/// stable one.
pub fn value_type_terms(listing: LibraryType, kind: SecKind) -> Vec<(String, String)> {
    listing.type_terms(kind)
}
