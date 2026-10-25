//! `LibraryView::filters`: the server-declared filters a viewer chose in one library's view, as
//! ONE additive field of the existing `library_sorts` record (`plex/CLAUDE.md`, "The contract for a
//! new setting"). A view without filters must serialize byte for byte as it always did, an older
//! build must carry the field through its own rewrite, a malformed element costs itself, and what
//! is remembered is bounded by bytes with WHOLE terms dropped, never cut.

use super::v0;
use super::*;
use serde_json::{json, Value};

const PROTECTED: &str = r#"{"format":"plxnative-session-auth","version":1,"account_token":"tok","server":{},"user":{},"home_users":[],"sources":[],"extensions":{}}"#;

fn value(key: &str, title: &str) -> StoredValue {
    StoredValue { key: key.into(), title: title.into() }
}

fn term(listing: &str, facet: &str, keys: &[&str]) -> StoredFilter {
    StoredFilter {
        listing: listing.into(),
        facet: facet.into(),
        flag: false,
        values: keys.iter().map(|k| value(k, &format!("title {k}"))).collect(),
    }
}

fn flag(listing: &str, facet: &str) -> StoredFilter {
    StoredFilter { listing: listing.into(), facet: facet.into(), flag: true, values: Vec::new() }
}

fn view_with(filters: Vec<StoredFilter>) -> LibraryView {
    LibraryView { machine_id: "machine".into(), key: 3, sort: "addedAt".into(), filters, ..Default::default() }
}

fn encoded(filters: &[StoredFilter]) -> usize {
    serde_json::to_string(filters).unwrap().len()
}

/// A realistic view: one list term with two values, one boolean, one term of the episodes listing.
fn typical() -> Vec<StoredFilter> {
    vec![
        term("", "actor", &["5780", "8810"]),
        flag("", "hdr"),
        term("episodes", "year", &["2024"]),
        term("", "studio", &["X%2520Y"]),
    ]
}

#[test]
fn a_pre_441_entry_serializes_byte_for_byte_with_the_new_field() {
    // the literal blob of `a_pre_441_library_sorts_entry_loads_as_a_sort_only_view`, and an entry
    // with every other optional field: neither grows a `filters` key
    let before = r#"[{"user":"u","libs":[{"machine_id":"machine","key":3,"sort":"viewCount","desc":true},{"machine_id":"m","key":4,"sort":"","desc":false,"unwatched":true,"genre":"28","listing":"episodes","listing_sort":"addedAt","listing_desc":true}]}]"#;
    let views: Vec<LibraryViews> = serde_json::from_str(before).unwrap();
    assert!(views[0].libs.iter().all(|view| view.filters.is_empty()));
    assert_eq!(serde_json::to_string(&views).unwrap(), before);
}

#[test]
fn filters_round_trip_through_split_and_join() {
    let mut session = Session::default();
    let view = view_with(typical());
    session.set_view_for("u", view.clone());
    let public = split_public(&session).unwrap();
    let stored = &public.preferences["library_sorts"][0]["libs"][0]["filters"];
    assert_eq!(
        stored,
        &json!([
            {"facet":"actor","values":[{"key":"5780","title":"title 5780"},{"key":"8810","title":"title 8810"}]},
            {"facet":"hdr","flag":true},
            {"listing":"episodes","facet":"year","values":[{"key":"2024","title":"title 2024"}]},
            {"facet":"studio","values":[{"key":"X%2520Y","title":"title X%2520Y"}]},
        ]),
        "a default listing, a false flag and an empty value list are omitted"
    );
    let joined = join_canonical(&public, PROTECTED).unwrap();
    for restored in [&joined, &public_session(&public)] {
        assert_eq!(restored.views_for("u").and_then(|views| views.get("machine", 3)), Some(&view));
    }
    let file: Session = serde_json::from_value(serde_json::to_value(&session).unwrap()).unwrap();
    assert_eq!(file.views_for("u").and_then(|views| views.get("machine", 3)), Some(&view));
}

#[test]
fn a_malformed_filter_element_costs_only_itself() {
    let file = json!({"client_id":"cid","library_sorts":[{"user":"u","libs":[
        {"machine_id":"machine","key":1,"sort":"addedAt","filters":"not a list"},
        {"machine_id":"machine","key":2,"filters":[7, null, "x", {"facet":"hdr","flag":true},
            {"facet":5}, {"facet":"actor","values":[{"key":"1"}, 9, null, {"key":2}, {"key":"3","title":"T"}]}]},
        {"machine_id":"machine","key":3,"filters":{"facet":"hdr"}}]}]});
    let session: Session = serde_json::from_value(file).unwrap();
    let views = session.views_for("u").unwrap();
    assert!(views.get("machine", 1).unwrap().filters.is_empty(), "a field that is not a list is no list");
    assert!(views.get("machine", 3).unwrap().filters.is_empty(), "an object is not a list either");
    let kept = &views.get("machine", 2).unwrap().filters;
    assert_eq!(kept.iter().map(|t| t.facet.as_str()).collect::<Vec<_>>(), ["hdr", "actor"],
        "the garbage elements and the term whose facet is not a string cost themselves");
    assert_eq!(kept[1].values, vec![value("1", ""), value("3", "T")], "a malformed value costs itself");
}

#[test]
fn an_older_build_keeps_filters_when_it_rewrites_the_record() {
    let mut session = Session::default();
    session.set_view_for("u", view_with(typical()));
    let written = serde_json::to_value(&session.prefs.library_views).unwrap();
    // the previous build's frozen types: they know nothing of `filters`, edit what they know …
    let mut old: Vec<v0::LibraryViews> = serde_json::from_value(written.clone()).unwrap();
    old[0].libs[0].genre = "28".into();
    old[0].libs[0].unwatched = true;
    let rewritten = serde_json::to_value(&old).unwrap();
    // … and write the new field back verbatim
    assert_eq!(rewritten[0]["libs"][0]["filters"], written[0]["libs"][0]["filters"]);
    assert_eq!(rewritten[0]["libs"][0]["genre"], json!("28"));
    // this build reads the old build's rewrite with both the edit and the filters
    let back: Vec<LibraryViews> = serde_json::from_value(rewritten).unwrap();
    assert_eq!(back[0].libs[0].filters, typical());
    assert_eq!(back[0].libs[0].genre, "28");
}

#[test]
fn a_view_with_filters_is_not_default() {
    assert!(LibraryView::default().is_default());
    assert!(!view_with(vec![flag("", "hdr")]).is_default());
    let only_filters = LibraryView { machine_id: "m".into(), key: 1, filters: vec![flag("", "hdr")], ..Default::default() };
    let mut views = LibraryViews::default();
    views.set(only_filters.clone());
    assert_eq!(views.get("m", 1), Some(&only_filters), "a view that is only filters is remembered");
}

#[test]
fn a_stored_facet_name_outside_the_charset_drops_its_term() {
    // a stored name is interpolated into a request path at restore
    let bad = ["a/b", "x?y=1", "", "ge nre", "a,b", "caf\u{e9}", "../up", &"n".repeat(49)];
    for name in bad {
        let mut views = LibraryViews::default();
        views.set(view_with(vec![term("", name, &["1"]), term("", "genre", &["2"])]));
        let kept: Vec<&str> = views.get("machine", 3).unwrap().filters.iter().map(|t| t.facet.as_str()).collect();
        assert_eq!(kept, ["genre"], "{name:?}");
    }
    // and on READ, from a file this build did not write
    let file = json!({"client_id":"cid","library_sorts":[{"user":"u","libs":[
        {"machine_id":"machine","key":3,"sort":"s","filters":[{"facet":"a/b","values":[{"key":"1"}]},{"facet":"ok","values":[{"key":"1"}]}]}]}]});
    let session: Session = serde_json::from_value(file).unwrap();
    let kept: Vec<&str> = session.views_for("u").unwrap().get("machine", 3).unwrap().filters.iter().map(|t| t.facet.as_str()).collect();
    assert_eq!(kept, ["ok"]);
}

#[test]
fn a_listing_that_is_not_a_listing_word_drops_its_terms_and_collections_carry_none() {
    let mut views = LibraryViews::default();
    views.set(view_with(vec![
        term("seasons", "year", &["1"]),
        term("collections", "year", &["1"]),
        term("elsewhere", "year", &["1"]),
        term("", "year", &["1"]),
    ]));
    let kept: Vec<&str> = views.get("machine", 3).unwrap().filters.iter().map(|t| t.listing.as_str()).collect();
    assert_eq!(kept, ["seasons", ""]);
}

#[test]
fn one_term_per_listing_and_facet_and_text_bounds_on_single_strings() {
    let mut views = LibraryViews::default();
    let long_title = "t".repeat(200);
    views.set(view_with(vec![
        term("", "genre", &["1"]),
        term("", "genre", &["2"]),
        StoredFilter { facet: "actor".into(), values: vec![value("5", &long_title), value("5", "again"), value("", "no key")], ..Default::default() },
        flag("", "hdr"),
        StoredFilter { facet: "inProgress".into(), flag: true, values: vec![value("1", "x")], ..Default::default() },
        StoredFilter { facet: "label".into(), ..Default::default() },
    ]));
    let kept = &views.get("machine", 3).unwrap().filters;
    assert_eq!(kept.iter().map(|t| t.facet.as_str()).collect::<Vec<_>>(), ["genre", "actor", "hdr", "inProgress"],
        "the first term of a facet wins; a term with no values and no flag is nothing");
    assert_eq!(kept[0].values, vec![value("1", "title 1")]);
    assert_eq!(kept[1].values.len(), 1, "a value with no key costs itself, a repeated key is one value");
    assert_eq!(kept[1].values[0].title.chars().count(), LibraryViews::MAX_FILTER_TITLE, "a title is cut");
    assert!(kept[3].values.is_empty(), "a boolean term holds no values");
}

#[test]
fn an_over_budget_term_is_dropped_whole_never_cut() {
    let key = "k".repeat(LibraryViews::MAX_FILTER_KEY);
    let fat = |facet: &str, n: usize| StoredFilter {
        facet: facet.into(),
        values: (0..n).map(|i| value(&format!("{i:03}{}", &key[3..]), "")).collect(),
        ..Default::default()
    };
    let terms = vec![term("", "genre", &["1", "2"]), fat("actor", 8), fat("studio", 8), flag("", "hdr")];
    assert!(encoded(&terms) > LibraryViews::MAX_FILTER_BYTES, "the fixture is over the budget");
    let mut views = LibraryViews::default();
    views.set(view_with(terms.clone()));
    let kept = &views.get("machine", 3).unwrap().filters;
    assert!(encoded(kept) <= LibraryViews::MAX_FILTER_BYTES, "{}", encoded(kept));
    assert_eq!(kept[..], terms[..kept.len()], "whole terms, in order, from the front: nothing was cut");
    assert!(kept.len() < terms.len() && !kept.is_empty());
    // a key over the bound drops its own term and nothing else
    let mut views = LibraryViews::default();
    views.set(view_with(vec![term("", "genre", &["1"]), term("", "actor", &[&"k".repeat(LibraryViews::MAX_FILTER_KEY + 1)])]));
    let kept: Vec<&str> = views.get("machine", 3).unwrap().filters.iter().map(|t| t.facet.as_str()).collect();
    assert_eq!(kept, ["genre"]);
}

#[test]
fn filters_are_bounded_by_bytes_on_set_and_on_read() {
    let big: Vec<StoredFilter> = (0..40).map(|i| term("", &format!("facet{i}"), &["1", "2", "3"])).collect();
    let mut views = LibraryViews::default();
    views.set(view_with(big.clone()));
    assert!(encoded(&views.get("machine", 3).unwrap().filters) <= LibraryViews::MAX_FILTER_BYTES);
    // a record this build did not write (or a hand edit) is bounded the same way when it is READ
    let file = json!({"client_id":"cid","library_sorts":[{"user":"u","libs":[
        {"machine_id":"machine","key":3,"sort":"s","filters":serde_json::to_value(&big).unwrap()}]}]});
    let session: Session = serde_json::from_value(file).unwrap();
    let read = &session.views_for("u").unwrap().get("machine", 3).unwrap().filters;
    assert!(!read.is_empty() && encoded(read) <= LibraryViews::MAX_FILTER_BYTES, "{}", encoded(read));
    assert_eq!(read[..], big[..read.len()]);
}

#[test]
fn a_real_shape_view_keeps_the_legacy_fields_beside_the_new_one() {
    // the A0 real-shape case: what this build writes for a view that has everything
    let mut session = Session::default();
    let view = LibraryView {
        machine_id: "m".into(), key: 2, sort: "titleSort".into(), unwatched: true, genre: "7".into(),
        listing: "episodes".into(), listing_sort: "addedAt".into(), filters: typical(), ..Default::default()
    };
    session.set_view_for("u", view.clone());
    let (public, protected) = split_canonical(&session).unwrap();
    let libs: &Value = &public.preferences["library_sorts"][0]["libs"][0];
    assert_eq!(libs["genre"], json!("7"), "the legacy fields are still written beside the new one");
    assert_eq!(libs["unwatched"], json!(true));
    assert_eq!(libs["filters"].as_array().map(Vec::len), Some(4));
    let joined = join_canonical(&public, &protected).unwrap();
    assert_eq!(joined.views_for("u").and_then(|views| views.get("m", 2)), Some(&view));
}
