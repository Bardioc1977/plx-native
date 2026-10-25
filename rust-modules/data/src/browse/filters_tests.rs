//! The filter model: a canonical set of terms, one per facet, compared by value KEYS (a title is
//! display), written to the wire as one parameter per facet with the keys comma-joined.

use super::filters::*;
use super::{LibraryType, SecKind};

fn value(id: &str, title: &str) -> FilterValue {
    FilterValue { id: id.into(), title: title.into() }
}

fn term(facet: &str, ids: &[&str]) -> FilterTerm {
    FilterTerm { facet: facet.into(), flag: false, values: ids.iter().map(|id| value(id, &format!("T{id}"))).collect() }
}

fn flag(facet: &str) -> FilterTerm {
    FilterTerm { facet: facet.into(), flag: true, values: Vec::new() }
}

fn ids(set: &FilterSet, facet: &str) -> Vec<String> {
    set.get(facet).map(|term| term.values.iter().map(|value| value.id.clone()).collect()).unwrap_or_default()
}

#[test]
fn a_filter_set_is_canonical_and_one_per_facet() {
    let set = FilterSet::from_terms([
        term("studio", &["9", "3", "9"]),
        flag("hdr"),
        term("genre", &["12", "7"]),
        term("genre", &["99"]),
        FilterTerm { facet: "label".into(), flag: false, values: Vec::new() },
    ]);
    assert_eq!(set.terms().iter().map(|t| t.facet.as_str()).collect::<Vec<_>>(), ["genre", "hdr", "studio"],
        "sorted by facet name, one per facet (the first wins), a term with nothing in it is no term");
    assert_eq!(ids(&set, "genre"), ["12", "7"], "values sorted by key");
    assert_eq!(ids(&set, "studio"), ["3", "9"], "and repeated keys collapse");
    assert_eq!(set.len(), 3);
    assert!(!set.is_empty() && FilterSet::default().is_empty());
}

#[test]
fn sets_compare_by_id_not_by_title() {
    let a = FilterSet::from_terms([term("genre", &["7"])]);
    let renamed = FilterSet::from_terms([FilterTerm { facet: "genre".into(), flag: false, values: vec![value("7", "Renamed")] }]);
    assert_eq!(a, renamed, "a renamed value or a refreshed title is not a different query");
    assert_ne!(a, FilterSet::from_terms([term("genre", &["8"])]));
    assert_ne!(a, FilterSet::from_terms([term("genre", &["7", "8"])]));
    assert_ne!(FilterSet::from_terms([flag("hdr")]), FilterSet::default());
}

#[test]
fn the_operations_return_canonical_sets() {
    let set = FilterSet::default()
        .with("genre", vec![value("12", "x"), value("7", "y")])
        .with_flag("hdr", true)
        .with("studio", vec![value("S", "s")]);
    assert_eq!(set.len(), 3);
    assert_eq!(set.get("hdr").map(|t| t.flag), Some(true));
    assert_eq!(set.without("genre").len(), 2);
    assert!(set.without("genre").get("genre").is_none());
    assert_eq!(set.with("genre", Vec::new()), set.without("genre"), "no values is no term");
    assert_eq!(set.with_flag("hdr", false), set.without("hdr"));
    assert_eq!(set.with("genre", vec![value("1", "")]).get("genre").unwrap().values.len(), 1, "replaces the facet's values");
    assert!(set.cleared().is_empty());
    let (kept, dropped) = set.retain_facets(|facet| facet != "hdr" && facet != "studio");
    assert_eq!((kept.len(), dropped), (1, vec!["hdr".to_owned(), "studio".to_owned()]));
}

#[test]
fn a_set_is_written_as_one_parameter_per_facet_with_the_keys_comma_joined() {
    let set = FilterSet::from_terms([term("studio", &["X%2520Y"]), term("genre", &["12", "7"]), flag("hdr")]);
    assert_eq!(set.wire(), vec![
        ("genre".to_owned(), "12,7".to_owned()),
        ("hdr".to_owned(), "1".to_owned()),
        ("studio".to_owned(), "X%2520Y".to_owned()),
    ], "canonical order; a boolean is `1`; keys verbatim");
    assert!(FilterSet::default().wire().is_empty());
}

#[test]
fn the_byte_count_is_what_the_request_line_carries() {
    let set = FilterSet::from_terms([term("genre", &["12", "7"]), flag("hdr")]);
    // `genre=12,7&hdr=1`: a `&` or `?` before each pair, and the keys as they go on the wire
    assert_eq!(set.wire_bytes(), "&genre=12,7&hdr=1".len());
    let encoded = FilterSet::from_terms([term("studio", &["a b"])]);
    assert_eq!(encoded.wire_bytes(), "&studio=a%20b".len(), "an unsafe byte costs three");
    assert_eq!(FilterSet::default().wire_bytes(), 0);
    assert!(MAX_QUERY_BYTES >= 4096);
}

#[test]
fn unwatched_names_its_facet_by_listing() {
    assert_eq!(unwatched_facet(SecKind::Movie, LibraryType::Primary), "unwatched");
    assert_eq!(unwatched_facet(SecKind::Show, LibraryType::Primary), "unwatchedLeaves");
    assert_eq!(unwatched_facet(SecKind::Show, LibraryType::Seasons), "unwatchedLeaves");
    assert_eq!(unwatched_facet(SecKind::Show, LibraryType::Episodes), "unwatched");
}

#[test]
fn the_value_path_is_the_listings_type_terms_for_picker_and_restore() {
    // `year` differs by type scope on a TV library (51 untyped, 20 with type=2, 50 with type=4), so
    // the one rule is: the facet name plus the listing's own type terms, from this one function.
    let pair = |ty: &str| vec![("type".to_owned(), ty.to_owned())];
    assert_eq!(value_type_terms(LibraryType::Primary, SecKind::Show), pair("2"));
    assert_eq!(value_type_terms(LibraryType::Episodes, SecKind::Show), pair("4"));
    assert_eq!(value_type_terms(LibraryType::Seasons, SecKind::Show), pair("3"));
    assert_eq!(value_type_terms(LibraryType::Primary, SecKind::Movie), Vec::<(String, String)>::new());
    for (listing, kind) in [(LibraryType::Primary, SecKind::Show), (LibraryType::Episodes, SecKind::Show), (LibraryType::Primary, SecKind::Movie)] {
        assert_eq!(value_type_terms(listing, kind), listing.type_terms(kind), "{listing:?} of {kind:?}");
    }
}

#[test]
fn a_reserved_or_unsafe_name_is_no_term() {
    let set = FilterSet::from_terms([
        term("type", &["1"]), term("sort", &["x"]), term("includeMeta", &["1"]), flag("unwatched"),
        flag("unwatchedLeaves"), term("a/b", &["1"]), term("", &["1"]), term("genre", &["7"]),
    ]);
    assert_eq!(set.terms().iter().map(|t| t.facet.as_str()).collect::<Vec<_>>(), ["genre"],
        "the request's own parameters and the Unwatched switch are not facets a viewer narrows by");
}
