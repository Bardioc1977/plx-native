//! What a restart restores of the viewer's filters: the Unwatched switch and every stored term of the
//! shown listing apply to the FIRST page's query (nothing is asked first, so there is no flash of the
//! unfiltered listing); only the genre is checked against its value list, as it always was; the page's
//! `Meta` then drops the terms of a facet the server no longer offers, asks the listing again, and
//! never lets a degraded answer erase the record or the Unwatched switch.

#![cfg(feature = "devtriggers")]

use super::*;
use plx_plex::plex::session::{StoredFilter, StoredValue};

/// A movie section's first page with the facets this server offers (no `unwatched` among them).
const MOVIE_PAGE: &str = r#"{"MediaContainer":{"totalSize":2,"Metadata":[{"ratingKey":"a-title"},{"ratingKey":"b-title"}],"Meta":{"Type":[{"active":true,"Sort":[{"key":"titleSort","title":"Title"}],"Filter":[{"filter":"genre","filterType":"string","title":"Genre"},{"filter":"actor","filterType":"string","title":"Actor"},{"filter":"hdr","filterType":"boolean","title":"HDR"},{"filter":"studio","filterType":"string","title":"Studio"}]}]}}}"#;
/// The same listing asked again without the facet that went (a different first row).
const AFTER_THE_DROP: &str = r#"{"MediaContainer":{"totalSize":2,"Metadata":[{"ratingKey":"unfiltered-hit"},{"ratingKey":"other"}]}}"#;
/// A server that answers 0 for a stale key: no error, an empty listing.
const NOTHING: &str = r#"{"MediaContainer":{"totalSize":0,"Metadata":[],"Meta":{"Type":[{"active":true,"Sort":[{"key":"titleSort","title":"Title"}],"Filter":[{"filter":"studio","filterType":"string"}]}]}}}"#;
/// A degraded answer: a `Meta` block whose menus are empty.
const NO_MENUS: &str = r#"{"MediaContainer":{"totalSize":2,"Metadata":[{"ratingKey":"a-title"},{"ratingKey":"b-title"}],"Meta":{"Type":[{"active":true,"Sort":[{"key":"titleSort","title":"Title"}],"Filter":[]}]}}}"#;
const SHOW_PAGE: &str = r#"{"MediaContainer":{"totalSize":1,"Metadata":[{"ratingKey":"a-show"}],"Meta":{"Type":[{"active":true,"Sort":[{"key":"titleSort","title":"Title"}],"Filter":[{"filter":"genre","filterType":"string"},{"filter":"hdr","filterType":"boolean"}]}]}}}"#;
const SEASONS_PAGE: &str = r#"{"MediaContainer":{"totalSize":1,"Metadata":[{"ratingKey":"a-season"}],"Meta":{"Type":[{"active":true,"Sort":[{"key":"show.titleSort,index","title":"Show"}]}]}}}"#;

fn term(listing: &str, facet: &str, keys: &[&str]) -> StoredFilter {
    StoredFilter {
        listing: listing.into(),
        facet: facet.into(),
        flag: false,
        values: keys.iter().map(|key| StoredValue { key: (*key).into(), title: format!("Title {key}") }).collect(),
    }
}

fn flag(listing: &str, facet: &str) -> StoredFilter {
    StoredFilter { listing: listing.into(), facet: facet.into(), flag: true, values: Vec::new() }
}

/// Put `view` in the session file of `user`, as a previous run left it, and sign `user` in.
fn remembered(label: &str, user: &str, view: LibraryView) -> TempPins {
    let session = TempPins::new(label);
    session.watching(user);
    plx_plex::plex::reset_servers_for_test();
    plx_plex::plex::session::update(move |current| {
        let mut next = current.clone();
        next.set_view_for(user, view.clone());
        Some(next)
    });
    session
}

fn view_of(filters: Vec<StoredFilter>) -> LibraryView {
    LibraryView { machine_id: "filter-machine".into(), key: 1, filters, ..Default::default() }
}

fn requests(pms: &Pms, n: usize) -> Vec<String> {
    let seen: Vec<String> = (0..n)
        .map(|i| pms.requests.recv_timeout(std::time::Duration::from_secs(1)).unwrap_or_else(|_| format!("NO REQUEST #{i}")))
        .collect();
    assert!(seen.iter().all(|line| !line.starts_with("NO REQUEST")), "the PMS was asked fewer than {n} times: {seen:?}");
    assert!(pms.requests.recv_timeout(std::time::Duration::from_millis(200)).is_err(), "no request beyond {n}: {seen:?}");
    seen
}

#[test]
fn a_stored_term_is_in_the_first_pages_query_and_nothing_is_asked_first() {
    let _g = plx_base::testlock::serial();
    let _session = remembered("filters-first-page", "u-filters", view_of(vec![term("", "actor", &["5780"]), flag("", "hdr")]));
    let _cleanup = RegisteredCleanup;
    let pms = loopback_pms("filter-machine", vec![MOVIE_PAGE]);
    let mut browse = movies_on(&pms, "filter-machine");
    pump_until_landed(&mut browse);

    let snapshot = browse.state.listing_snapshot();
    let view = snapshot.view();
    assert_eq!(view.item(0).map(|item| item.rk.as_str()), Some("a-title"));
    let asked = requests(&pms, 1);
    assert!(asked[0].contains("actor=5780") && asked[0].contains("hdr=1"), "{}", asked[0]);
    let applied = view.filters();
    assert_eq!(applied.get("actor").map(|t| t.values[0].title.as_str()), Some("Title 5780"), "the stored title shows at once");
    assert_eq!(applied.get("hdr").map(|t| t.flag), Some(true));
    let state = browse.state.cur_state().unwrap();
    assert!(state.facets_known && state.facets.iter().any(|f| f.name == "actor"), "the page's Meta supplied the facets");
}

#[test]
fn only_the_genre_is_checked_against_its_list_every_other_term_is_trusted() {
    let _g = plx_base::testlock::serial();
    let mut view = view_of(vec![term("", "actor", &["5780"])]);
    view.genre = "28".into();
    let _session = remembered("filters-genre-and-terms", "u-filters", view);
    let _cleanup = RegisteredCleanup;
    let pms = loopback_pms("filter-machine", vec![GENRE_DIR, MOVIE_PAGE]);
    let mut browse = movies_on(&pms, "filter-machine");
    pump_until_landed(&mut browse);

    let asked = requests(&pms, 2);
    assert!(asked[0].contains("/library/sections/1/genre") && asked[0].contains("X-Plex-Container-Size=500"),
        "one read, in a page of 500, before the first page: {}", asked[0]);
    assert!(asked[1].contains("genre=28") && asked[1].contains("actor=5780"), "{}", asked[1]);
    let snapshot = browse.state.listing_snapshot();
    assert_eq!(snapshot.view().filters().len(), 2);
    assert_eq!(snapshot.view().genre().map(|genre| genre.title.as_str()), Some("Action"));
}

#[test]
fn a_stale_key_matches_nothing_and_the_term_stays_for_the_viewer_to_clear() {
    let _g = plx_base::testlock::serial();
    let _session = remembered("filters-stale", "u-filters", view_of(vec![term("", "studio", &["gone"])]));
    let _cleanup = RegisteredCleanup;
    let pms = loopback_pms("filter-machine", vec![NOTHING]);
    let mut browse = movies_on(&pms, "filter-machine");
    pump_until_landed(&mut browse);

    let snapshot = browse.state.listing_snapshot();
    assert_eq!(snapshot.view().total(), 0, "the server answered 0, not an error");
    assert_eq!(snapshot.view().filters().get("studio").map(|t| t.values.len()), Some(1),
        "a stale key is not condemned unseen: the empty read-out with Clear filters is the net");
    let _ = requests(&pms, 1);
}

#[test]
fn an_unadvertised_facet_is_dropped_and_the_page_asked_again() {
    let _g = plx_base::testlock::serial();
    let _session = remembered("filters-unadvertised", "u-filters",
        view_of(vec![term("", "network", &["77"]), term("", "actor", &["5780"])]));
    let _cleanup = RegisteredCleanup;
    let pms = loopback_pms("filter-machine", vec![MOVIE_PAGE, AFTER_THE_DROP]);
    let mut browse = movies_on(&pms, "filter-machine");
    pump_until_landed(&mut browse);
    // the page asked with the stored term lands with the menu that proves it gone; the listing is asked again
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while browse.state.cur_state().is_some_and(|state| state.total < 0 || state.items.get(0).is_none_or(|item| item.rk == "a-title"))
        && std::time::Instant::now() < deadline {
        let _ = browse.pump();
        browse.run_listing_jobs();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let asked = requests(&pms, 2);
    assert!(asked[0].contains("network=77") && asked[0].contains("actor=5780"), "{}", asked[0]);
    assert!(!asked[1].contains("network=") && asked[1].contains("actor=5780"), "{}", asked[1]);
    let snapshot = browse.state.listing_snapshot();
    assert_eq!(snapshot.view().item(0).map(|item| item.rk.as_str()), Some("unfiltered-hit"),
        "the page asked with the stale term was never published");
    assert!(snapshot.view().filters().get("network").is_none() && snapshot.view().filters().get("actor").is_some());
}

#[test]
fn an_empty_facet_list_never_drops_a_term_and_a_degraded_meta_does_not_erase_the_record() {
    let _g = plx_base::testlock::serial();
    let _session = remembered("filters-degraded", "u-filters", view_of(vec![term("", "network", &["77"])]));
    let _cleanup = RegisteredCleanup;
    let pms = loopback_pms("filter-machine", vec![NO_MENUS, MOVIE_PAGE]);
    let mut browse = movies_on(&pms, "filter-machine");
    pump_until_landed(&mut browse);
    let state = browse.state.cur_state().unwrap();
    assert!(!state.facets_known, "an empty list proves nothing");
    assert_eq!(state.filters.len(), 1, "so nothing was dropped, and the listing was not asked again");
    // an unrelated edit leaves the record as it was
    switch_unwatched_on(&mut browse, &pms);
    let held = stored_view("u-filters", "filter-machine").expect("the record is still there");
    assert!(held.unwatched);
    assert_eq!(held.filters, vec![term("", "network", &["77"])]);
    let _ = requests(&pms, 2);
}

#[test]
fn terms_survive_a_restart_per_listing_and_the_unwatched_switch_is_never_reconciled() {
    let _g = plx_base::testlock::serial();
    let mut view = view_of(vec![flag("", "hdr"), term("episodes", "year", &["2024"])]);
    view.unwatched = true;
    let _session = remembered("filters-per-listing", "u-filters", view);
    let _cleanup = RegisteredCleanup;
    // the shows listing's menu names no `unwatchedLeaves`: the switch applies all the same
    let pms = loopback_pms("filter-machine", vec![SHOW_PAGE]);
    let mut browse = library_on(&pms, "filter-machine", "TV", SecKind::Show);
    pump_until_landed(&mut browse);
    let asked = requests(&pms, 1);
    assert!(asked[0].contains("type=2") && asked[0].contains("unwatchedLeaves=1") && asked[0].contains("hdr=1"), "{}", asked[0]);
    assert!(!asked[0].contains("year="), "the episodes' term belongs to the episodes listing: {}", asked[0]);
    let snapshot = browse.state.listing_snapshot();
    assert!(snapshot.view().unwatched(), "the Meta reconcile never touches the Unwatched switch");
    let state = browse.state.cur_state().unwrap();
    assert_eq!(state.facets.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["genre", "hdr", "unwatchedLeaves"],
        "a server that omits the Unwatched facet still gets the row, under the show's name");
    // the viewer moves to the episodes: their own term comes back, the shows' does not follow
    assert!(browse.state.set_library_type(LibraryType::Episodes));
    let episodes = &browse.state.cur_state().unwrap().filters;
    assert_eq!(episodes.get("year").map(|t| t.values[0].id.as_str()), Some("2024"));
    assert!(episodes.get("hdr").is_none());
}

#[test]
fn seasons_advertise_no_facet_and_still_get_the_unwatched_row() {
    let _g = plx_base::testlock::serial();
    let mut view = view_of(vec![]);
    view.listing = "seasons".into();
    view.unwatched = true;
    let _session = remembered("filters-seasons", "u-filters", view);
    let _cleanup = RegisteredCleanup;
    let pms = loopback_pms("filter-machine", vec![SEASONS_PAGE]);
    let mut browse = library_on(&pms, "filter-machine", "TV", SecKind::Show);
    pump_until_landed(&mut browse);
    let asked = requests(&pms, 1);
    assert!(asked[0].contains("type=3") && asked[0].contains("unwatchedLeaves=1"), "{}", asked[0]);
    let state = browse.state.cur_state().unwrap();
    assert_eq!(state.facets.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["unwatchedLeaves"]);
    assert!(state.facets_known);
}
