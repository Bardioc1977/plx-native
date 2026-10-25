//! The section's query state holds a `FilterSet`: the grid's page, the A-Z letters read and its key,
//! and the Collections loan all take the same terms from `SecState::query_filters`; terms belong to
//! one listing type; an edit that changes nothing changes nothing.

use super::filters::*;
use super::*;

fn value(id: &str, title: &str) -> FilterValue {
    FilterValue { id: id.into(), title: title.into() }
}

fn facet(name: &str, boolean: bool) -> plx_plex::plex::Facet {
    plx_plex::plex::Facet {
        name: name.into(),
        kind: if boolean { plx_plex::plex::FacetKind::Boolean } else { plx_plex::plex::FacetKind::List },
        title: name.into(),
    }
}

/// A browse state sitting on the TV library of the two-server fixture (section index 1).
fn on_the_tv_library() -> BrowseState {
    let mut state = BrowseState::default();
    seed_two_source_table_for_owner_test(&mut state);
    state.set_cur(1);
    assert_eq!(state.sections[1].kind, SecKind::Show);
    state
}

fn selection() -> FilterSet {
    FilterSet::default()
        .with("genre", vec![value("12", "Drama"), value("7", "Comedy")])
        .with_flag("hdr", true)
        .with("studio", vec![value("X%2520Y", "X Y")])
}

fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

#[test]
fn the_page_the_letters_and_the_loan_carry_the_same_terms() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    assert!(state.set_unwatched(true));
    let gen = state.query_gen();
    assert!(state.set_filters(selection()));
    assert_ne!(state.query_gen(), gen, "a changed selection asks the listing afresh");
    let want = vec![pair("type", "2"), pair("unwatchedLeaves", "1"), pair("genre", "12,7"), pair("hdr", "1"),
        pair("studio", "X%2520Y")];
    // the page: `type=` first, then the Unwatched switch, then the terms in canonical order
    assert_eq!(state.states[1].query_filters(SecKind::Show), want);
    // the letters are asked with those very terms (the key is built from the same state) …
    let key = state.states[1].letters_key();
    state.states[1].land_letters(key, vec![("A".into(), 3)]);
    assert!(state.states[1].letters_current());
    // … a refreshed title is not another query …
    let renamed = FilterSet::default()
        .with("genre", vec![value("12", "Renamed"), value("7", "Comedy")])
        .with_flag("hdr", true)
        .with("studio", vec![value("X%2520Y", "X Y")]);
    state.states[1].filters = Arc::new(renamed);
    assert!(state.states[1].letters_current(), "the letters' key compares keys, not titles");
    // … and another selection is
    state.states[1].filters = Arc::new(selection().with_flag("hdr", false));
    assert!(!state.states[1].letters_current(), "the list counted a different listing");
    assert!(state.states[1].letters_owed());
    // the view reads the selection AS APPLIED
    assert!(state.set_filters(selection()));
    assert_eq!(state.listing_snapshot().view().filters(), &selection());
}

#[test]
fn an_edit_equal_to_the_live_set_is_a_no_op() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    assert!(state.set_filters(selection()));
    let gen = state.query_gen();
    let same_with_new_titles = FilterSet::default()
        .with("genre", vec![value("7", "Comedy!"), value("12", "Drama!")])
        .with_flag("hdr", true)
        .with("studio", vec![value("X%2520Y", "Other title")]);
    assert!(state.set_filters(same_with_new_titles), "an equal selection is a landed edit");
    assert_eq!(state.query_gen(), gen, "…that asks nothing again");
    assert!(state.set_filters(selection().without("hdr")));
    assert_ne!(state.query_gen(), gen);
}

#[test]
fn terms_do_not_cross_listing_types() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    assert!(state.set_unwatched(true));
    assert!(state.set_filters(selection()));
    assert!(state.set_library_type(LibraryType::Episodes));
    assert!(state.states[1].filters.is_empty(), "a listing starts with no terms of another's");
    assert_eq!(state.states[1].query_filters(SecKind::Show), vec![pair("type", "4"), pair("unwatched", "1")],
        "…while the Unwatched switch is the section's own and carries across");
    assert!(state.set_filters(FilterSet::default().with_flag("hdr", true)));
    assert_eq!(state.states[1].query_filters(SecKind::Show),
        vec![pair("type", "4"), pair("unwatched", "1"), pair("hdr", "1")]);
}

#[test]
fn collections_send_and_show_no_terms_and_the_loan_returns_the_set_whole() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    state.states[1].facets = Arc::new(vec![facet("genre", false), facet("hdr", true), facet("studio", false)]);
    state.states[1].facets_known = true;
    assert!(state.set_unwatched(true));
    assert!(state.set_filters(selection()));
    assert!(state.set_library_type(LibraryType::Collections));
    assert_eq!(state.states[1].query_filters(SecKind::Show), vec![pair("type", "18")]);
    assert!(state.listing_snapshot().view().filters().is_empty(), "not shown while Collections are listed");
    assert!(!state.set_filters(FilterSet::default().with_flag("hdr", true)),
        "Collections have no facets: an edit there is refused");
    assert!(state.set_library_type(LibraryType::Primary));
    assert_eq!(*state.states[1].filters, selection(), "the loan returns the selection whole");
    assert_eq!(state.states[1].facets.len(), 3, "and the facets");
    assert!(state.states[1].facets_known);
    assert_eq!(state.states[1].query_filters(SecKind::Show).len(), 6);
}

#[test]
fn unwatched_in_progress_and_leaves_depend_on_viewing() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    assert!(!state.states[1].membership_depends_on_viewing());
    assert!(state.set_filters(FilterSet::default().with_flag("hdr", true)));
    assert!(!state.states[1].membership_depends_on_viewing(), "a quality facet does not change when something is watched");
    assert!(state.set_filters(FilterSet::default().with_flag("inProgress", true)));
    assert!(state.states[1].membership_depends_on_viewing(), "what is in progress changes as it is watched");
    assert!(state.set_filters(FilterSet::default()));
    assert!(state.set_unwatched(true));
    assert!(state.states[1].membership_depends_on_viewing());
}

#[test]
fn seasons_keep_the_measured_unwatched_toggle() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    assert!(state.set_library_type(LibraryType::Seasons));
    assert!(state.states[1].facets.is_empty(), "a season type advertises no facet");
    assert!(state.set_unwatched(true));
    assert_eq!(state.states[1].query_filters(SecKind::Show), vec![pair("type", "3"), pair("unwatchedLeaves", "1")]);
}

#[test]
fn a_set_over_the_query_budget_is_refused() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    let big = FilterSet::default().with("actor", (0..120).map(|i| value(&format!("{i:040}"), "n")).collect());
    assert!(big.wire_bytes() > MAX_QUERY_BYTES);
    let gen = state.query_gen();
    assert!(!state.set_filters(big), "the request line cannot carry it");
    assert_eq!(state.query_gen(), gen);
    assert!(state.states[1].filters.is_empty());
    let fits = FilterSet::default().with("actor", (0..30).map(|i| value(&format!("{i:040}"), "n")).collect());
    assert!(fits.wire_bytes() <= MAX_QUERY_BYTES);
    assert!(state.set_filters(fits));
}

#[test]
fn terms_for_a_facet_the_known_list_lacks_are_dropped_by_an_edit() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    // before the menus landed nothing is known: the edit is taken as it is
    assert!(state.set_filters(FilterSet::default().with_flag("hdr", true)));
    assert_eq!(state.states[1].filters.len(), 1);
    state.states[1].facets = Arc::new(vec![facet("genre", false), facet("studio", false)]);
    state.states[1].facets_known = true;
    assert!(state.set_filters(selection()));
    assert_eq!(state.states[1].filters.terms().iter().map(|t| t.facet.as_str()).collect::<Vec<_>>(), ["genre", "studio"],
        "`hdr` is not offered here");
}

#[test]
fn the_genre_is_the_first_value_of_the_genre_term() {
    let _guard = plx_base::testlock::serial();
    let mut state = on_the_tv_library();
    assert!(state.set_filters(FilterSet::default().with("genre", vec![value("7", "Comedy")])));
    let listing = state.listing_snapshot();
    assert_eq!(listing.view().genre().map(|genre| (genre.id.as_str(), genre.title.as_str())), Some(("7", "Comedy")));
    assert!(state.set_library_type(LibraryType::Episodes));
    assert!(state.listing_snapshot().view().genre().is_none());
}
