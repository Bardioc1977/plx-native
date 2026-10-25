//! A facet's value list as a store job: paged, single flight, scoped by the listing's type terms and
//! by nothing else, landed APPENDED in the server's order, refused when it is for another type or
//! another generation or out of order, kept for the three facets opened last, and the one source of
//! the genre list (`LibraryWork::Genres` is "all pages of `genre`").

use super::*;
use crate::browse::filters::{FilterSet, FilterValue, VALUE_PAGE};
use crate::browse::SecKind;
use crate::browse::test_support::*;
use crate::stores::browse::{LibraryWork, SectionAddress};
use crate::stores::StoreJob;

fn value(id: &str, title: &str) -> FilterValue {
    FilterValue { id: id.into(), title: title.into() }
}

/// The directory job the steps have left, taken off the outbox (`None`: there is none).
fn job_left(browse: &mut TestBrowse) -> Option<(plx_machine::job::Job<BrowseDirectoryReq>, BrowseDirectoryPort)> {
    // the pump also asks for the listing's first page; this file is about the value reads
    let mut jobs: Vec<StoreJob> = std::mem::take(&mut browse.jobs)
        .into_iter().filter(|job| matches!(job, StoreJob::BrowseDirectory(..))).collect();
    match jobs.len() {
        0 => None,
        1 => match jobs.remove(0) {
            StoreJob::BrowseDirectory(job, port) => Some((job, port)),
            other => panic!("a directory job, not {other:?}"),
        },
        n => panic!("{n} jobs on the outbox: {jobs:?}"),
    }
}

/// The picker's tick: `Values { facet, want_rows }` on the registry as of now.
fn ask(browse: &mut TestBrowse, sid: plx_plex::plex::ServerId, facet: &str, want_rows: usize) {
    tick(browse, sid, LibraryWork::Values { facet: facet.into(), want_rows });
}

fn tick(browse: &mut TestBrowse, sid: plx_plex::plex::ServerId, work: LibraryWork) {
    browse.refill();
    let target = SectionAddress { epoch: browse.state.table_epoch(), sid, section: 1 };
    assert!(browse.state.addressed_with_adapter(&browse.adapter, target, work, &browse.keys, &mut browse.jobs));
}

/// What the server answers for a window of `n` rows named `{prefix}{start+i}`, with a `totalSize`.
fn window(prefix: &str, start: usize, n: usize, total: Option<usize>) -> MediaContainer {
    let rows: Vec<String> = (start..start + n).map(|i| format!(r#"{{"key":"{prefix}{i}","title":"T {prefix}{i}"}}"#)).collect();
    let total = total.map(|t| format!(r#","totalSize":{t},"offset":{start}"#)).unwrap_or_default();
    serde_json::from_str(&format!(r#"{{"size":{}{total},"Directory":[{}]}}"#, n, rows.join(","))).unwrap()
}

/// Run the job's body against `answer` and land it on the pump.
fn land(browse: &mut TestBrowse, client: &'static Client,
    (job, port): (plx_machine::job::Job<BrowseDirectoryReq>, BrowseDirectoryPort), answer: Option<MediaContainer>) {
    finish_directory_with(&port, job.addr, client, job.req, move || answer);
    let _ = browse.pump();
}

/// [`land`] for the one job the last tick left.
fn land_next(browse: &mut TestBrowse, client: &'static Client, answer: Option<MediaContainer>) {
    let held = job_left(browse).expect("a read was owed");
    land(browse, client, held, answer);
}

fn rows_of(browse: &TestBrowse, facet: &str) -> Vec<String> {
    browse.state.states()[0].value_list(facet).map(|list| list.rows.iter().map(|row| row.id.clone()).collect()).unwrap_or_default()
}

#[test]
fn a_value_read_carries_no_terms_and_the_listings_type_terms() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _client) = loopback_source_at(9, SecKind::Show);
    assert!(browse.state.set_unwatched(true));
    assert!(browse.state.set_filters(FilterSet::default().with("genre", vec![value("7", "Comedy")])));
    ask(&mut browse, sid, "year", 50);
    let (job, _port) = job_left(&mut browse).expect("a value read is owed");
    assert_eq!(job.req.facet(), Some("year"));
    assert_eq!(job.req.start(), 0);
    assert_eq!(job.req.filters, vec![("type".to_owned(), "2".to_owned())],
        "the listing's own type, and neither Unwatched nor the genre: the whole stable list");
    assert_eq!(job.req.filters, crate::browse::filters::value_type_terms(LibraryType::Primary, SecKind::Show),
        "…from the one rule the restore also asks with");
}

#[test]
fn value_reads_are_single_flight_and_a_refused_one_settles() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "studio", 50);
    let (job, port) = job_left(&mut browse).expect("owed");
    assert!(Directory::Values.busy(&browse.adapter), "the step raised the claim");
    ask(&mut browse, sid, "studio", 50);
    ask(&mut browse, sid, "actor", 50);
    assert!(job_left(&mut browse).is_none(), "one value read is out at a time, whichever facet asks");
    port.refused(job.addr);
    assert!(!Directory::Values.busy(&browse.adapter), "the refusal gave the claim back");
    ask(&mut browse, sid, "studio", 50);
    let (again, _port) = job_left(&mut browse).expect("a read that never started is asked for again");
    assert_eq!((again.req.facet(), again.req.start()), (Some("studio"), 0));
    assert_ne!(again.addr, job.addr);
}

#[test]
fn a_value_page_lands_appended_in_order_until_the_list_is_complete() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "actor", 1000);
    land_next(&mut browse, client, Some(window("a", 0, VALUE_PAGE, Some(250))));
    assert_eq!(rows_of(&browse, "actor").len(), 100);
    assert!(!browse.state.states()[0].value_list("actor").unwrap().complete);
    ask(&mut browse, sid, "actor", 1000);
    let (job, port) = job_left(&mut browse).expect("the next window is owed");
    assert_eq!(job.req.start(), 100, "pages append: the next window starts where the rows end");
    land(&mut browse, client, (job, port), Some(window("a", 100, VALUE_PAGE, Some(250))));
    ask(&mut browse, sid, "actor", 1000);
    let (job, port) = job_left(&mut browse).unwrap();
    assert_eq!(job.req.start(), 200);
    land(&mut browse, client, (job, port), Some(window("a", 200, 50, Some(250))));
    let list = browse.state.states()[0].value_list("actor").unwrap();
    assert_eq!((list.rows.len(), list.total, list.complete), (250, Some(250), true));
    assert_eq!(list.rows[0].id, "a0");
    assert_eq!(list.rows[249].id, "a249", "in the server's order");
    assert_eq!(list.rows[7].title, "T a7");
    ask(&mut browse, sid, "actor", 1000);
    assert!(job_left(&mut browse).is_none(), "a complete list is not asked for again");
}

#[test]
fn the_picker_asks_only_as_far_as_it_wants() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "actor", 20);
    land_next(&mut browse, client, Some(window("a", 0, VALUE_PAGE, Some(250))));
    ask(&mut browse, sid, "actor", 20);
    assert!(job_left(&mut browse).is_none(), "100 rows are on hand and 20 are wanted");
    ask(&mut browse, sid, "actor", 101);
    assert!(job_left(&mut browse).is_some(), "101 are wanted");
}

#[test]
fn a_short_page_ends_a_list_that_reports_no_total() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "country", 50);
    land_next(&mut browse, client, Some(window("c", 0, 3, None)));
    let list = browse.state.states()[0].value_list("country").unwrap();
    assert_eq!((list.rows.len(), list.complete), (3, true));
    ask(&mut browse, sid, "label", 50);
    land_next(&mut browse, client, serde_json::from_str(r#"{"size":0}"#).ok());
    let list = browse.state.states()[0].value_list("label").unwrap();
    assert!(list.rows.is_empty() && list.complete, "an empty list is a landed, complete list");
}

#[test]
fn a_value_page_for_another_type_or_generation_is_dropped() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Show);
    ask(&mut browse, sid, "year", 50);
    let held = job_left(&mut browse).unwrap();
    assert!(browse.state.set_library_type(LibraryType::Episodes), "the viewer moved to another listing");
    land(&mut browse, client, held, Some(window("y", 0, 5, Some(5))));
    assert!(browse.state.states()[0].value_list("year").is_none(), "the show years are not the episodes'");
    assert!(!Directory::Values.busy(&browse.adapter), "but the take released the claim");
    ask(&mut browse, sid, "year", 50);
    let (job, _port) = job_left(&mut browse).expect("the new listing asks for its own");
    assert_eq!(job.req.filters, vec![("type".to_owned(), "4".to_owned())]);
}

#[test]
fn an_out_of_order_value_page_is_dropped() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "actor", 1000);
    land_next(&mut browse, client, Some(window("a", 0, VALUE_PAGE, Some(250))));
    // a stale duplicate of the first window arrives after it
    let stale = BrowseDirectoryReq::new(browse.state.section_sid(0).unwrap(), ClientKey::of(client),
        browse.state.table_epoch(), 0, 1, LibraryType::Primary, Vec::new(), None, Directory::Values,
        Some(ValuesAsk { facet: "actor".into(), start: 0, size: VALUE_PAGE, generation: browse.state.states()[0].values_gen, total: None, fetched: 0 }));
    let port = BrowseDirectoryPort::of(&browse.adapter, Directory::Values);
    Directory::Values.claim(&browse.adapter, 99);
    finish_directory_with(&port, listing_addr(99), client, stale, || Some(window("a", 0, VALUE_PAGE, Some(250))));
    let _ = browse.pump();
    assert_eq!(rows_of(&browse, "actor").len(), 100, "rows 0..100 are not appended a second time");
}

#[test]
fn a_reset_drops_value_lists_and_their_claim() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "studio", 50);
    land_next(&mut browse, client, Some(window("s", 0, 3, None)));
    assert!(browse.state.states()[0].value_list("studio").is_some());
    ask(&mut browse, sid, "actor", 50);
    assert!(Directory::Values.busy(&browse.adapter));
    browse.reset();
    assert!(browse.state.states().is_empty(), "the lists go with the section table");
    assert!(!Directory::Values.busy(&browse.adapter), "and a claim latched by a read that will never land");
}

#[test]
fn the_cache_keeps_three_lists_the_most_recently_opened_first() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    for facet in ["studio", "actor", "country", "director"] {
        ask(&mut browse, sid, facet, 50);
        land_next(&mut browse, client, Some(window(facet, 0, 2, None)));
    }
    let kept: Vec<&str> = browse.state.states()[0].values.iter().map(|(facet, _)| facet.as_str()).collect();
    assert_eq!(kept, ["director", "country", "actor"], "the list opened first was let go");
    ask(&mut browse, sid, "studio", 50);
    let (job, _port) = job_left(&mut browse).expect("a list let go is read again, from its start");
    assert_eq!((job.req.facet(), job.req.start()), (Some("studio"), 0));
}

#[test]
fn selected_titles_follow_a_landing_and_a_complete_list_prunes_a_stale_key() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    assert!(browse.state.set_filters(FilterSet::default().with("studio", vec![value("s1", "Old title"), value("gone", "Stale")])));
    let gen = browse.state.query_gen();
    ask(&mut browse, sid, "studio", 50);
    land_next(&mut browse, client, Some(window("s", 0, 3, Some(3))));
    let studio = browse.state.states()[0].filters.get("studio").unwrap().clone();
    assert_eq!(studio.values.iter().map(|v| (v.id.as_str(), v.title.as_str())).collect::<Vec<_>>(), [("s1", "T s1")],
        "the title is the server's now, and a complete list that does not hold a key drops it");
    assert_ne!(browse.state.query_gen(), gen, "the listing was filtered by a key that matches nothing: it is asked afresh");
}

#[test]
fn a_partial_list_never_prunes() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    assert!(browse.state.set_filters(FilterSet::default().with("studio", vec![value("s1", "Old title"), value("far", "Not yet seen")])));
    let gen = browse.state.query_gen();
    ask(&mut browse, sid, "studio", 50);
    land_next(&mut browse, client, Some(window("s", 0, VALUE_PAGE, Some(400))));
    let studio = browse.state.states()[0].filters.get("studio").unwrap().clone();
    assert_eq!(studio.values.len(), 2, "the key may be on a page that has not landed");
    assert_eq!(studio.values.iter().find(|v| v.id == "s1").map(|v| v.title.as_str()), Some("T s1"), "but titles are refreshed");
    assert_eq!(browse.state.query_gen(), gen);
}

#[test]
fn a_boolean_facet_and_an_unsafe_name_are_never_asked_for() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _client) = loopback_source_at(9, SecKind::Movie);
    {
        let state = browse.state.state_mut(0).unwrap();
        state.facets = Arc::new(vec![
            plx_plex::plex::Facet { name: "unwatched".into(), kind: plx_plex::plex::FacetKind::Boolean, title: "Unwatched".into() },
            plx_plex::plex::Facet { name: "studio".into(), kind: plx_plex::plex::FacetKind::List, title: "Studio".into() },
        ]);
        state.facets_known = true;
    }
    for facet in ["unwatched", "a/b", "", "x?y=1"] {
        ask(&mut browse, sid, facet, 50);
        assert!(job_left(&mut browse).is_none(), "{facet:?}: the key of `unwatched` answers the whole listing");
    }
    ask(&mut browse, sid, "studio", 50);
    assert!(job_left(&mut browse).is_some());
}

#[test]
fn the_genre_work_is_all_pages_of_the_genre_list() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    tick(&mut browse, sid, LibraryWork::Genres);
    let (job, port) = job_left(&mut browse).expect("the genre list is owed");
    assert_eq!((job.req.facet(), job.req.start()), (Some("genre"), 0));
    land(&mut browse, client, (job, port), Some(window("g", 0, VALUE_PAGE, Some(130))));
    assert_eq!(browse.state.genres().len(), 100, "the old accessor reads the same list");
    tick(&mut browse, sid, LibraryWork::Genres);
    let (job, port) = job_left(&mut browse).expect("a list still short of its total is asked for again, with no row count");
    assert_eq!(job.req.start(), 100);
    land(&mut browse, client, (job, port), Some(window("g", 100, 30, Some(130))));
    assert_eq!(browse.state.genres().len(), 130);
    assert!(browse.state.states()[0].genres_done());
    tick(&mut browse, sid, LibraryWork::Genres);
    assert!(job_left(&mut browse).is_none());
}

#[test]
fn a_failed_value_read_backs_off_and_is_asked_again() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "studio", 50);
    land_next(&mut browse, client, None);
    let list = browse.state.states()[0].value_list("studio").expect("a failed read leaves a marker, not rows");
    assert!(list.rows.is_empty() && !list.complete && list.failed.is_some());
    ask(&mut browse, sid, "studio", 50);
    assert!(job_left(&mut browse).is_none(), "the same list is held back for a while");
    for _ in 0..crate::browse::letters::LETTERS_RETRY_STEPS {
        let _ = browse.pump();
    }
    ask(&mut browse, sid, "studio", 50);
    assert!(job_left(&mut browse).is_some(), "…and asked for again once the back-off is spent");
}

/// A window like [`window`], but its row 5 has a key no request line carries (120 bytes: a studio
/// name encoded twice costs five bytes a byte), so `FacetPage` skips it from the rows.
fn window_with_a_long_key(prefix: &str, start: usize, n: usize, total: Option<usize>) -> MediaContainer {
    let rows: Vec<String> = (start..start + n)
        .map(|i| if i == start + 5 { format!(r#"{{"key":"{}","title":"long"}}"#, "k".repeat(120)) }
            else { format!(r#"{{"key":"{prefix}{i}","title":"T {prefix}{i}"}}"#) })
        .collect();
    let total = total.map(|t| format!(r#","totalSize":{t},"offset":{start}"#)).unwrap_or_default();
    serde_json::from_str(&format!(r#"{{"size":{}{total},"Directory":[{}]}}"#, n, rows.join(","))).unwrap()
}

#[test]
fn a_skipped_row_does_not_shift_the_next_window() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "studio", 1000);
    land_next(&mut browse, client, Some(window_with_a_long_key("s", 0, VALUE_PAGE, Some(250))));
    assert_eq!(rows_of(&browse, "studio").len(), 99, "the row no request can carry is not offered");
    ask(&mut browse, sid, "studio", 1000);
    let (job, port) = job_left(&mut browse).expect("the next window is owed");
    assert_eq!(job.req.start(), 100, "…but the server's next window starts after the 100 rows it sent");
    land(&mut browse, client, (job, port), Some(window("s", 100, VALUE_PAGE, Some(250))));
    ask(&mut browse, sid, "studio", 1000);
    let (job, port) = job_left(&mut browse).unwrap();
    assert_eq!(job.req.start(), 200);
    land(&mut browse, client, (job, port), Some(window("s", 200, 50, Some(250))));
    let list = browse.state.states()[0].value_list("studio").unwrap();
    assert!(list.complete, "250 rows were consumed, whatever of them were offered");
    let keys = rows_of(&browse, "studio");
    assert_eq!(keys.len(), 249);
    assert_eq!(keys.iter().collect::<std::collections::BTreeSet<_>>().len(), keys.len(), "no value twice");
}

#[test]
fn a_skipped_row_does_not_end_a_list_that_reports_no_total() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_source_at(9, SecKind::Movie);
    ask(&mut browse, sid, "studio", 1000);
    land_next(&mut browse, client, Some(window_with_a_long_key("s", 0, VALUE_PAGE, None)));
    let list = browse.state.states()[0].value_list("studio").unwrap();
    assert_eq!(list.rows.len(), 99);
    assert!(!list.complete, "a window of 100 rows asked for and 100 sent is not a short page");
    ask(&mut browse, sid, "studio", 1000);
    let (job, _port) = job_left(&mut browse).expect("the list goes on");
    assert_eq!(job.req.start(), 100);
}

#[test]
fn a_switch_and_a_request_parameter_are_refused_by_name_before_the_menu_has_landed() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _client) = loopback_source_at(9, SecKind::Movie);
    assert!(!browse.state.states()[0].facets_known, "premise: no Meta yet, so no facet is known to be boolean");
    for name in ["unwatched", "unwatchedLeaves", "type", "sort", "includeMeta"] {
        ask(&mut browse, sid, name, 50);
        assert!(job_left(&mut browse).is_none(), "{name}: its key answers a whole listing or is the request's own");
    }
    ask(&mut browse, sid, "studio", 50);
    assert!(job_left(&mut browse).is_some());
}
