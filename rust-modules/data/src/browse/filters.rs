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

/// Boolean facets whose membership changes as items are watched, beside the Unwatched switch
/// itself (the names `unwatched` and `unwatchedLeaves` are the switch): marking an item watched
/// while one is on makes the listing stale ([`super::SecState::membership_depends_on_viewing`]).
pub const FILTERS_DEPENDING_ON_VIEWING: [&str; 1] = ["inProgress"];

impl FilterSet {
    /// The empty selection, for a place that needs a reference to one.
    pub const fn empty() -> FilterSet {
        FilterSet { terms: Vec::new() }
    }

    /// The canonical set of `terms`.
    pub fn from_terms(terms: impl IntoIterator<Item = FilterTerm>) -> Self {
        let mut kept: Vec<FilterTerm> = Vec::new();
        for mut term in terms {
            if kept.iter().any(|seen| seen.facet == term.facet) || !plx_plex::plex::valid_term_name(&term.facet) {
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

// ---- the stored form, and what a page's Meta does to the selection ----------------------------

use plx_plex::plex::session::{StoredFilter, StoredValue};
use plx_plex::plex::{Facet, FacetKind};

impl FilterSet {
    /// The selection a record holds for `listing`: the terms of that listing, as stored. The library's
    /// own listing's genre is not among them: the legacy `genre` field of the record is the one store
    /// of it (an older build edits that field and keeps `filters` as it was, so a genre kept in both
    /// would be resurrected by a stale copy).
    pub(super) fn from_stored(stored: &[StoredFilter], listing: LibraryType) -> FilterSet {
        FilterSet::from_terms(
            stored
                .iter()
                .filter(|term| term.listing == listing.wire())
                .filter(|term| !(listing == LibraryType::Primary && term.facet == "genre"))
                .map(|term| FilterTerm {
                    facet: term.facet.clone(),
                    flag: term.flag,
                    values: term
                        .values
                        .iter()
                        .map(|value| FilterValue { id: value.key.clone(), title: value.title.clone() })
                        .collect(),
                }),
        )
    }

    /// This selection as the record stores it for `listing`, without the own listing's genre (the
    /// legacy `genre` field is its one store).
    pub(super) fn to_stored(&self, listing: LibraryType) -> Vec<StoredFilter> {
        self.terms
            .iter()
            .filter(|term| !(listing == LibraryType::Primary && term.facet == "genre"))
            .map(|term| StoredFilter {
                listing: listing.wire().to_owned(),
                facet: term.facet.clone(),
                flag: term.flag,
                values: term
                    .values
                    .iter()
                    .map(|value| StoredValue { key: value.id.clone(), title: value.title.clone() })
                    .collect(),
            })
            .collect()
    }
}

impl super::SecState {
    /// The first page's `Meta` offered `offered` for this listing. Returns whether it proved a term of
    /// the selection gone, which is then dropped and the listing must be asked again.
    ///
    /// * The Unwatched facet is synthesised, not merely read: a server that omits it (or a season type,
    ///   which advertises none) still gets the row, under the listing's own name
    ///   ([`unwatched_facet`]). The switch is not a term and is never reconciled.
    /// * An EMPTY list proves nothing for a listing that normally has facets: one degraded answer must
    ///   not erase what the viewer chose, so nothing is learned and nothing dropped. (Seasons and
    ///   Collections legitimately advertise none.)
    pub(super) fn land_facets(&mut self, kind: Option<SecKind>, offered: Vec<Facet>) -> bool {
        let listing = self.library_type;
        if offered.is_empty() && !matches!(listing, LibraryType::Seasons | LibraryType::Collections) {
            return false;
        }
        let mut offered = offered;
        if let (true, Some(kind)) = (listing.filters(), kind) {
            let name = unwatched_facet(kind, listing);
            if !offered.iter().any(|facet| facet.name == name) {
                offered.push(Facet { name: name.to_owned(), kind: FacetKind::Boolean, title: "Unwatched".to_owned() });
            }
        }
        let facets = std::sync::Arc::new(offered);
        let (kept, dropped) = self.filters.retain_facets(|facet| facets.iter().any(|offered| offered.name == facet));
        self.facets = facets;
        self.facets_known = true;
        if dropped.is_empty() {
            return false;
        }
        self.filters = std::sync::Arc::new(kept);
        true
    }
}
