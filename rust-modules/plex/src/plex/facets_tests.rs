//! The server's filter facets: what `Meta.Type[].Filter[]` parses to, the request paths of a
//! facet's value list, and how a filter term goes on the wire. Shapes are those measured on
//! PMS 1.43.5 (`docs/library-filters-spec.md`); every string here is synthetic.

use super::client::QueryBuilder;
use super::facets::{valid_facet_name, Facet, FacetKind, FacetPage};
use super::library::{facet_values_path, section_items_path};
use super::models::MediaContainer;
use super::paging::PageReq;
use super::params::SectionQuery;

const MOVIE: [(&str, &str); 28] = [
    ("genre", "string"), ("year", "integer"), ("decade", "integer"), ("contentRating", "string"),
    ("collection", "string"), ("director", "string"), ("actor", "string"), ("writer", "string"),
    ("producer", "string"), ("country", "string"), ("studio", "string"), ("resolution", "string"),
    ("hdr", "boolean"), ("dovi", "boolean"), ("hdr10plus", "boolean"), ("atmos", "boolean"),
    ("unwatched", "boolean"), ("inProgress", "boolean"), ("unmatched", "boolean"),
    ("videoCodec", "string"), ("audioCodec", "string"), ("subtitleCodec", "string"),
    ("audioLayout", "string"), ("audioLanguage", "string"), ("subtitleLanguage", "string"),
    ("editionTitle", "string"), ("label", "string"), ("location", "string"),
];

fn record(name: &str, filter_type: &str) -> String {
    format!(
        r#"{{"filter":"{name}","filterType":"{filter_type}","key":"/library/sections/1/{name}","title":"T {name}","type":"filter"}}"#
    )
}

fn container(types: &[(&str, bool, Option<String>)]) -> MediaContainer {
    let types: Vec<String> = types
        .iter()
        .map(|(kind, active, filters)| {
            let filter = filters.as_ref().map(|f| format!(r#","Filter":{f}"#)).unwrap_or_default();
            format!(r#"{{"key":"/library/sections/1/all?type=1","type":"{kind}","title":"{kind}","active":{active}{filter},"Sort":[]}}"#)
        })
        .collect();
    let json = format!(r#"{{"MediaContainer":{{"size":0,"Meta":{{"Type":[{}]}}}}}}"#, types.join(","));
    let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
    serde_json::from_value(wrapper["MediaContainer"].clone()).unwrap()
}

fn facets_of(filters: &str) -> Vec<Facet> {
    let mc = container(&[("movie", true, Some(filters.to_owned()))]);
    mc.meta.unwrap().types[0].facets()
}

fn names(facets: &[Facet]) -> Vec<&str> {
    facets.iter().map(|f| f.name.as_str()).collect()
}

fn query<'a>(filters: &'a [(String, String)]) -> SectionQuery<'a> {
    SectionQuery { section_key: 1, sort: "", filters, start: 0, size: 60, include_meta: false }
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

fn wire(v: &str) -> String {
    QueryBuilder::new("/x").wire("k", v).build()
}

#[test]
fn a_movie_meta_block_yields_28_facets_in_server_order() {
    let filters = format!("[{}]", MOVIE.iter().map(|(n, t)| record(n, t)).collect::<Vec<_>>().join(","));
    let facets = facets_of(&filters);
    assert_eq!(facets.len(), 28);
    assert_eq!(names(&facets), MOVIE.iter().map(|(n, _)| *n).collect::<Vec<_>>());
    for (facet, (name, filter_type)) in facets.iter().zip(MOVIE) {
        let want = if filter_type == "boolean" { FacetKind::Boolean } else { FacetKind::List };
        assert_eq!(facet.kind, want, "{name}");
        assert_eq!(facet.title, format!("T {name}"));
    }
}

#[test]
fn a_season_type_advertises_no_facet() {
    let mc = container(&[("show", true, Some(format!("[{}]", record("genre", "string")))), ("season", false, None)]);
    let types = mc.meta.unwrap().types;
    assert_eq!(types[0].facets().len(), 1);
    assert!(types[1].facets().is_empty());
    let null = container(&[("season", false, Some("null".into()))]);
    assert!(null.meta.unwrap().types[0].facets().is_empty(), "a null list is no list");
}

#[test]
fn a_facet_record_is_read_leniently() {
    let facets = facets_of(
        r#"[7, null, "x", {"filter":"year","filterType":"integer","title":"  Year \u0007 "},
            {"filterType":"boolean"}, {"filter":"","filterType":"string"},
            {"filter":"decade","filterType":"date"}, {"filter":"hdr","filterType":"BOOLEAN"},
            {"filter":"year","filterType":"integer"}, {"filter":"label"}, {"filter":"quality","title":null}]"#,
    );
    assert_eq!(names(&facets), ["year", "decade", "hdr", "label", "quality"],
        "garbage elements and nameless records cost themselves; the first of a repeated name wins");
    assert_eq!(facets[0].title, "Year", "trimmed, control characters stripped");
    assert_eq!(facets[1].kind, FacetKind::List, "a filterType this build does not know is still a list");
    assert_eq!(facets[2].kind, FacetKind::Boolean);
    assert_eq!(facets[3].title, "label", "an empty title falls back to the facet name");
    assert_eq!(facets[4].title, "quality");
    let long = facets_of(&format!(r#"[{{"filter":"genre","title":"{}"}}]"#, "w".repeat(200)));
    assert_eq!(long[0].title.chars().count(), 64, "a title is capped");
}

#[test]
fn a_facet_name_outside_the_safe_set_is_dropped() {
    for good in ["genre", "show.genre", "a_b.c", "A1", &"n".repeat(48)] {
        assert!(valid_facet_name(good), "{good}");
    }
    for bad in ["", "ge nre", "a/b", "a?b", "a=b", "a&b", "a%20b", "gen\u{e9}re", "a,b", "../x", &"n".repeat(49)] {
        assert!(!valid_facet_name(bad), "{bad}");
    }
    let facets = facets_of(r#"[{"filter":"a/b"},{"filter":"ok"},{"filter":"x?y=1"}]"#);
    assert_eq!(names(&facets), ["ok"]);
}

#[test]
fn the_value_path_is_built_from_name_and_type_never_from_the_servers_key() {
    let page = PageReq { start: 0, size: 100 };
    assert_eq!(
        facet_values_path(1, "year", FacetKind::List, &pairs(&[("type", "2")]), page).as_deref(),
        Some("/library/sections/1/year?type=2&X-Plex-Container-Start=0&X-Plex-Container-Size=100"),
        "a TV listing's value path is the facet name plus the listing's own type term",
    );
    assert_eq!(
        facet_values_path(7, "genre", FacetKind::List, &[], PageReq { start: 200, size: 500 }).as_deref(),
        Some("/library/sections/7/genre?X-Plex-Container-Start=200&X-Plex-Container-Size=500"),
    );
    // The record's `key` is not even parsed: a hostile one has nowhere to go.
    let facets = facets_of(r#"[{"filter":"genre","filterType":"string","key":"http://evil.example/steal?x="}]"#);
    assert_eq!(facets[0].name, "genre");
    let path = facet_values_path(1, &facets[0].name, facets[0].kind, &[], page).unwrap();
    assert!(path.starts_with("/library/sections/1/genre?"), "{path}");
    assert!(!path.contains("evil"), "{path}");
}

#[test]
fn a_boolean_facet_has_no_value_path() {
    // `GET /library/sections/1/unwatched` answers the whole unwatched LISTING (248 KB on a
    // 400-film library): the request must not be made, whatever else is true of it.
    let page = PageReq { start: 0, size: 100 };
    for name in ["unwatched", "hdr"] {
        assert_eq!(facet_values_path(1, name, FacetKind::Boolean, &[], page), None, "{name}");
    }
    assert_eq!(facet_values_path(1, "a/b", FacetKind::List, &[], page), None, "a name outside the safe set");
    assert_eq!(facet_values_path(1, "", FacetKind::List, &[], page), None);
    assert!(facet_values_path(1, "unwatched", FacetKind::List, &[], page).is_some(),
        "the kind the CALLER knows decides: a name alone is not a verdict");
}

#[test]
fn filter_keys_are_sent_verbatim() {
    // The server's keys arrive already percent-encoded (a studio is a name encoded twice); sending
    // them through the encoder again turns `%2520` into `%252520`, which matches nothing.
    let filters = pairs(&[("studio", "X%2520Y"), ("contentRating", "Not%20Rated"), ("audioLayout", "5%2E1")]);
    assert_eq!(
        section_items_path(&query(&filters)),
        "/library/sections/1/all?studio=X%2520Y&contentRating=Not%20Rated&audioLayout=5%2E1\
         &X-Plex-Container-Start=0&X-Plex-Container-Size=60",
    );
    assert_eq!(wire("199%2520Productions"), "/x?k=199%2520Productions");
    assert_eq!(wire("Not%20Rated"), "/x?k=Not%20Rated");
}

#[test]
fn a_hostile_key_is_still_one_parameter() {
    assert_eq!(wire("x&y=1#z"), "/x?k=x%26y%3D1%23z");
    assert_eq!(wire("a b+c"), "/x?k=a%20b%2Bc");
    assert_eq!(wire("100%"), "/x?k=100%25");
    assert_eq!(wire("%zz%2"), "/x?k=%25zz%252");
    assert_eq!(wire("caf\u{e9}"), "/x?k=caf%C3%A9");
    assert_eq!(wire("a>b"), "/x?k=a%3Eb");
    assert_eq!(wire("\u{7}\n"), "/x?k=%07%0A");
}

#[test]
fn a_term_is_one_parameter_with_comma_joined_keys() {
    let filters = pairs(&[("type", "2"), ("genre", "7,12"), ("resolution", "4k")]);
    assert_eq!(
        section_items_path(&query(&filters)),
        "/library/sections/1/all?type=2&genre=7,12&resolution=4k&X-Plex-Container-Start=0&X-Plex-Container-Size=60",
    );
}

#[test]
fn a_key_with_an_encoded_comma_does_not_become_two_values() {
    // The server decodes twice: `%2C` is the separator after the FIRST decode, so a single-encoded
    // comma inside one key would split it. `%252C` survives the first decode as a literal comma.
    // Exercised against the mock only; not verified on a real server (docs/library-filters-spec.md).
    assert_eq!(wire("A%2CB"), "/x?k=A%252CB");
    assert_eq!(wire("A%2cB"), "/x?k=A%252CB");
    assert_eq!(wire("A%252CB"), "/x?k=A%252CB", "already double-encoded: verbatim");
    assert_eq!(wire("7,12"), "/x?k=7,12", "a raw comma is the OR separator");
}

#[test]
fn existing_callers_keep_their_request_strings() {
    let filters = pairs(&[("type", "2"), ("unwatchedLeaves", "1"), ("genre", "150")]);
    let q = SectionQuery { sort: "addedAt:desc", include_meta: true, start: 60, ..query(&filters) };
    assert_eq!(
        section_items_path(&q),
        "/library/sections/1/all?includeMeta=1&sort=addedAt%3Adesc&type=2&unwatchedLeaves=1&genre=150\
         &X-Plex-Container-Start=60&X-Plex-Container-Size=60",
    );
    assert_eq!(section_items_path(&query(&[])),
        "/library/sections/1/all?X-Plex-Container-Start=0&X-Plex-Container-Size=60");
}

#[test]
fn facet_values_always_pages() {
    let path = facet_values_path(1, "actor", FacetKind::List, &[], PageReq { start: 0, size: 100 }).unwrap();
    assert!(path.contains("X-Plex-Container-Start=0"), "a Start of 0 is still sent: {path}");
    assert!(path.contains("X-Plex-Container-Size=100"), "{path}");
}

#[test]
fn a_value_page_is_read_into_rows() {
    let mc: MediaContainer = serde_json::from_str(
        r#"{"size":4,"totalSize":807,"offset":100,"Directory":[
            {"key":"5780","title":"Ann"},{"key":"","title":"no key"},{"key":"9","title":""},
            {"key":"a,b","title":"raw comma"},{"key":"X%2520Y","title":"X Y","fastKey":"/ignored"}]}"#,
    )
    .unwrap();
    let page = FacetPage::from_container(mc);
    assert_eq!(page.total, Some(807));
    let rows: Vec<(&str, &str)> = page.rows.iter().map(|r| (r.key.as_str(), r.title.as_str())).collect();
    assert_eq!(rows, [("5780", "Ann"), ("9", "9"), ("X%2520Y", "X Y")],
        "a row with no key or with a raw comma is skipped, an empty title falls back to the key");
    let empty: MediaContainer = serde_json::from_str(r#"{"size":0}"#).unwrap();
    let page = FacetPage::from_container(empty);
    assert!(page.rows.is_empty());
    assert_eq!(page.total, None, "an empty list has no Directory key and no total");
}

#[test]
fn a_skipped_row_is_still_a_consumed_row() {
    // The window the server sent is what the NEXT window starts after: a row that could not travel
    // as one value (over-long, a comma, a control character) is skipped from the rows but not from
    // the count, or the next ask would start one row early and list a value twice.
    let long = "k".repeat(120);
    let mc: MediaContainer = serde_json::from_str(&format!(
        r#"{{"size":4,"totalSize":250,"Directory":[{{"key":"1","title":"a"}},{{"key":"{long}","title":"long"}},
            {{"key":"x,y","title":"comma"}},{{"key":"2","title":"b"}}]}}"#,
    ))
    .unwrap();
    let page = FacetPage::from_container(mc);
    assert_eq!(page.rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["1", "2"]);
    assert_eq!(page.fetched, 4, "four rows were consumed from the server's list");
}

#[test]
fn a_reserved_name_is_no_term() {
    use super::facets::valid_term_name;
    for ok in ["genre", "actor", "show.genre", "hdr"] {
        assert!(valid_term_name(ok), "{ok}");
    }
    // names the request already carries, and the Unwatched switch, which is not a term and whose key
    // answers the whole listing
    for bad in ["type", "sort", "includeMeta", "unwatched", "unwatchedLeaves", "a/b", ""] {
        assert!(!valid_term_name(bad), "{bad}");
    }
    assert!(valid_facet_name("unwatched"), "…while it is still a facet the server may advertise");
}
