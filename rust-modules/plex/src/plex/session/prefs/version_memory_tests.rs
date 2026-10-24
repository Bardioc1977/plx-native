//! The data layer of per-item version memory: the entry and its bounds, the soft parse, the account
//! stamp, the forget rule, the per-profile cap and the household budget. The document it lives in
//! (the DB8 cap, the guard's shed order, sign-out) is `track_memory_document_tests`.
use super::super::{CanonicalSessionPreferences, Session};
use super::version_memory::*;
use super::{
    list_size, logical, MemoryEntry, Parse, Preferences, ProfileList, ProfileMemory, Scope, DESCRIPTORS,
};
use serde_json::json;

const MACHINE: &str = "0123456789abcdef0123456789abcdef01234567";
/// A Plex Home user's `uuid` is shorter than this; the worst case is a UUID-length one.
const LONG_USER: &str = "01234567-89ab-cdef-0123-456789abcdef";
use crate::plex::session::test_support::STAMP;
use crate::plex::session::prefs::track_memory::{MACHINE_ID_MAX, SHOW_RK_MAX};

fn entry(n: u32) -> VersionMemoryEntry {
    VersionMemoryEntry::new(MACHINE, &n.to_string(), 7_000_000 + i64::from(n))
}

fn key_of(e: &VersionMemoryEntry) -> (String, String) {
    e.key()
}

/// Write `e` for (`user`, `STAMP`) through the door.
fn put(list: &mut Vec<ProfileList<VersionMemoryEntry>>, user: &str, e: VersionMemoryEntry) -> bool {
    list.upsert_with(user, STAMP, &key_of(&e), |_| Some(e.clone()))
}

fn items_of(list: &[ProfileList<VersionMemoryEntry>], user: &str) -> Vec<String> {
    list.iter().find(|r| r.user == user).map(|r| r.items.iter().map(|e| e.rating_key.clone()).collect()).unwrap_or_default()
}

// ---- the row ---------------------------------------------------------------------------------

/// The list row is a `Preferences` field under its own key, written only while non-empty, and its
/// descriptor says what the plan says (per item, account-bound, not live, no Settings row).
#[test]
fn the_list_row_names_its_own_field() {
    let default_json = serde_json::to_value(Preferences::default()).unwrap();
    assert!(default_json.get("version_memory").is_none(), "an empty list writes no key: {default_json}");
    let mut p = Preferences::default();
    let mut list = Vec::new();
    assert!(put(&mut list, "", entry(1)));
    p.version_memory = list;
    let body = serde_json::to_value(&p).unwrap();
    assert!(body["version_memory"].is_array(), "{body}");
    let back: Preferences = serde_json::from_value(body).unwrap();
    assert_eq!(back, p);
    let row = DESCRIPTORS.iter().find(|d| d.key == "version_memory").expect("the row is in the table");
    assert_eq!(row.scope, Scope::PerItem);
    assert!(row.account_bound && !row.live && row.settings.is_none());
    assert_eq!(row.parse, Parse::SoftVec);
    assert_eq!(<VersionMemoryEntry as MemoryEntry>::CAP, 128);
    assert_eq!(<VersionMemoryEntry as MemoryEntry>::BUDGET, Some(24 * 1024));
    assert!(<VersionMemoryEntry as MemoryEntry>::ACCOUNT_BOUND && !<VersionMemoryEntry as MemoryEntry>::ADOPTS_UNSTAMPED);
}

#[test]
fn the_entry_serializes_the_documented_shape() {
    assert_eq!(
        serde_json::to_value(VersionMemoryEntry::new("m1", "42", 7_000_001)).unwrap(),
        json!({"machine_id": "m1", "rating_key": "42", "media_id": 7_000_001})
    );
}

// ---- the soft parse ---------------------------------------------------------------------------

/// **T-parse for the list row.** Junk is dropped per element, at both levels (a record, an entry),
/// in the struct on its own AND inside both of its on-disk hosts; the good elements survive.
#[test]
fn junk_elements_are_dropped_per_element() {
    let good = |rk: &str, id: i64| json!({"machine_id": MACHINE, "rating_key": rk, "media_id": id});
    let body = json!({"version_memory": [
        "junk", 7, null, {"user": "u1", "account": STAMP, "items": 3}, // records that are not records
        {"user": "u2", "account": STAMP, "items": [
            good("1", 7_000_001),
            7, "x", null,                                                                              // not objects
            {"machine_id": MACHINE, "rating_key": "2"},                                                 // no id
            {"machine_id": MACHINE, "rating_key": "3", "media_id": 0},                                  // the default needs no record
            {"machine_id": MACHINE, "rating_key": "4", "media_id": -5},                                 // not an id
            {"machine_id": MACHINE, "rating_key": "5", "media_id": "7000001"},                          // wrong type
            {"machine_id": "", "rating_key": "6", "media_id": 1},                                       // no server
            {"machine_id": MACHINE, "rating_key": "", "media_id": 1},                                   // no item
            {"machine_id": MACHINE, "rating_key": "r".repeat(SHOW_RK_MAX + 1), "media_id": 1},          // key over its bound
            {"machine_id": "m".repeat(MACHINE_ID_MAX + 1), "rating_key": "7", "media_id": 1},           // key over its bound
            {"rating_key": "8", "media_id": 1},                                                         // key missing
            good("9", 7_000_009),
        ]},
    ]});
    let alone: Preferences = serde_json::from_value(body.clone()).unwrap();
    let legacy: Session = serde_json::from_value(body.clone()).unwrap();
    let canonical: CanonicalSessionPreferences = serde_json::from_value(body).unwrap();
    for (host, list) in [
        ("struct", &alone.version_memory),
        ("legacy", &legacy.prefs.version_memory),
        ("canonical", &canonical.prefs.version_memory),
    ] {
        let u2 = list.iter().find(|r| r.user == "u2").unwrap_or_else(|| panic!("{host}: the good record survives"));
        assert_eq!(u2.items.iter().map(|e| e.rating_key.as_str()).collect::<Vec<_>>(), ["1", "9"], "{host}");
        assert_eq!(u2.items.iter().map(|e| e.media_id).collect::<Vec<_>>(), [7_000_001, 7_000_009], "{host}");
        assert!(u2.account == STAMP, "{host}");
        assert!(list.iter().all(|r| r.user == "u2" || r.items.is_empty()), "{host}: {list:?}");
    }
}

/// A key string over its bound is dropped (the entry is keyless and so forgotten), kept at it; the
/// write path and the read path are one normaliser.
#[test]
fn the_key_bounds_hold_on_read_and_write() {
    let at = VersionMemoryEntry::new(&"m".repeat(MACHINE_ID_MAX), &"9".repeat(SHOW_RK_MAX), 5);
    assert_eq!((at.machine_id.len(), at.rating_key.len()), (MACHINE_ID_MAX, SHOW_RK_MAX));
    assert!(!at.forgets());
    let long_machine = VersionMemoryEntry::new(&"m".repeat(MACHINE_ID_MAX + 1), "1", 5);
    let long_key = VersionMemoryEntry::new("m", &"9".repeat(SHOW_RK_MAX + 1), 5);
    assert!(long_machine.machine_id.is_empty() && long_machine.forgets());
    assert!(long_key.rating_key.is_empty() && long_key.forgets());
    // through the door a keyless or default entry records nothing
    let mut list = Vec::new();
    assert!(!put(&mut list, "", long_machine) || list.iter().all(|r| r.items.is_empty()));
    assert!(list.iter().all(|r| r.items.is_empty()));
}

/// **The default needs no record.** Writing an entry whose `media_id` is not a version (0: the
/// picker's "back to the default") removes the entry for its key, and `None` does too; neither
/// creates a record for a profile that has none.
#[test]
fn a_non_positive_id_or_none_forgets_the_entry() {
    let mut list = Vec::new();
    assert!(put(&mut list, "u", entry(1)));
    assert!(put(&mut list, "u", entry(2)));
    let forget = VersionMemoryEntry { media_id: 0, ..entry(1) };
    assert!(forget.forgets());
    assert!(put(&mut list, "u", forget));
    assert_eq!(items_of(&list, "u"), ["2"], "only the forgotten key went");
    assert!(list.upsert_with("u", STAMP, &key_of(&entry(2)), |_| None));
    assert!(list.is_empty(), "an emptied record is dropped, never kept empty: {list:?}");
    assert!(!list.upsert_with("nobody", STAMP, &key_of(&entry(3)), |_| None), "forgetting what is not there writes nothing");
    assert!(list.is_empty(), "and creates no record");
}

/// Two servers can share a `ratingKey`; the memory key includes the machine id, so a pick on one
/// never reaches the other, and a re-pick of one replaces only its own.
#[test]
fn two_servers_with_the_same_rating_key_do_not_collide() {
    let mut list = Vec::new();
    let a = VersionMemoryEntry::new("machine-a", "42", 111);
    let b = VersionMemoryEntry::new("machine-b", "42", 222);
    assert!(put(&mut list, "", a.clone()));
    assert!(put(&mut list, "", b.clone()));
    assert_eq!(list[0].items.len(), 2);
    assert_eq!(list.get("", STAMP, &key_of(&a)).map(|e| e.media_id), Some(111));
    assert_eq!(list.get("", STAMP, &key_of(&b)).map(|e| e.media_id), Some(222));
    assert!(put(&mut list, "", VersionMemoryEntry::new("machine-a", "42", 333)));
    assert_eq!(list.get("", STAMP, &key_of(&b)).map(|e| e.media_id), Some(222), "machine-b is untouched");
    assert_eq!(list.get("", STAMP, &key_of(&a)).map(|e| e.media_id), Some(333));
}

/// A pick belongs to a profile: the owner's record and a managed user's are separate, a pick by one
/// is invisible to the other, and forgetting one profile's entry leaves the other's.
#[test]
fn profiles_do_not_see_each_others_picks() {
    let mut list = Vec::new();
    let item = ("machine-a", "42");
    let key = (item.0.to_string(), item.1.to_string());
    assert!(put(&mut list, "", VersionMemoryEntry::new(item.0, item.1, 111)));
    assert!(put(&mut list, "kid", VersionMemoryEntry::new(item.0, item.1, 222)));
    assert_eq!(list.get("", STAMP, &key).map(|e| e.media_id), Some(111));
    assert_eq!(list.get("kid", STAMP, &key).map(|e| e.media_id), Some(222));
    assert!(list.get("someone-else", STAMP, &key).is_none());
    assert!(list.upsert_with("kid", STAMP, &key, |_| None));
    assert_eq!(list.get("", STAMP, &key).map(|e| e.media_id), Some(111), "the owner's pick stays");
    assert!(list.get("kid", STAMP, &key).is_none());
}

// ---- the account stamp ------------------------------------------------------------------------

/// A record stamped for another account reads as nothing, a write PRUNES it (and an unstamped
/// record nobody can read), and a write without a stamp is refused outright.
#[test]
fn a_stale_stamp_is_ignored_and_a_write_prunes_it() {
    let stale = "ffffffffffffffff";
    let mut list: Vec<ProfileList<VersionMemoryEntry>> = vec![
        ProfileList { user: "".into(), account: stale.into(), items: vec![entry(1)], ..Default::default() },
        ProfileList { user: "u2".into(), account: String::new(), items: vec![entry(2)], ..Default::default() },
    ];
    let key = key_of(&entry(1));
    assert!(list.get("", STAMP, &key).is_none(), "the other account's record is not this account's");
    assert!(list.get("", "", &key).is_none(), "an unstamped read finds nothing");
    assert!(list.get("u2", "", &key_of(&entry(2))).is_none(), "even an unstamped record is unreadable");
    assert!(list.get("", stale, &key).is_some(), "control: its own account reads it");

    assert!(put(&mut list, "", entry(3)));
    assert_eq!(list.len(), 1, "the foreign and the unstamped record are pruned: {list:?}");
    assert_eq!(list[0].account, STAMP);
    assert_eq!(items_of(&list, ""), ["3"]);

    let mut list: Vec<ProfileList<VersionMemoryEntry>> = Vec::new();
    let mut asked = false;
    assert!(!list.upsert_with("", "", &key, |_| { asked = true; Some(entry(1)) }));
    assert!(list.is_empty() && !asked, "a write without a stamp records nothing and is not even asked");
}

// ---- the per-profile cap ----------------------------------------------------------------------

#[test]
fn the_cap_of_128_drains_the_oldest() {
    let mut list = Vec::new();
    for n in 0..128 {
        assert!(put(&mut list, "", entry(n)));
    }
    assert_eq!(list[0].items.len(), 128);
    // re-recording the oldest moves it to the end instead of growing the list
    assert!(put(&mut list, "", VersionMemoryEntry::new(MACHINE, "0", 9_999)));
    assert_eq!(list[0].items.len(), 128);
    assert_eq!(list[0].items.last().map(|e| (e.rating_key.as_str(), e.media_id)), Some(("0", 9_999)));
    // a 129th item drains the OLDEST (now item 1), not the one just re-recorded
    assert!(put(&mut list, "", entry(128)));
    let items = items_of(&list, "");
    assert_eq!(items.len(), 128);
    assert!(!items.contains(&"1".to_string()) && items.contains(&"0".to_string()) && items.contains(&"128".to_string()), "{items:?}");
}

// ---- the household budget ---------------------------------------------------------------------

/// **One profile keeps 128 worst-case entries**, both key strings at their bound and the widest id,
/// with a UUID-length profile key and a stamp: the arithmetic the bounds exist for.
#[test]
fn one_profile_keeps_128_worst_case_entries() {
    let worst = VersionMemoryEntry::worst_case;
    assert_eq!(worst(0).machine_id.len(), MACHINE_ID_MAX);
    assert_eq!(worst(0).rating_key.len(), SHOW_RK_MAX, "the fixture's item key is at its bound");
    let mut list = Vec::new();
    for n in 0..128 {
        assert!(put(&mut list, LONG_USER, worst(n)));
    }
    let size = list_size(&list);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].items.len(), 128, "the budget evicted a lone viewer's entries ({size} of {BUDGET_BYTES} bytes)");
    assert!(size <= BUDGET_BYTES, "{size} > {BUDGET_BYTES}");
    // the figures the bounds are chosen against, so a bound that grows shows here first
    assert_eq!(logical(&worst(0)), WORST_ENTRY_BYTES);
    assert_eq!(size, WORST_PROFILE_BYTES, "128 worst-case entries are {WORST_PROFILE_BYTES} bytes ({} free)", BUDGET_BYTES - size);
    // the 129th is drained by the cap, not by the budget, and the list still fits
    assert!(put(&mut list, LONG_USER, worst(128)));
    assert_eq!(list[0].items.len(), 128);
    assert!(list_size(&list) <= BUDGET_BYTES);
}

/// The worst-case entry's logical size, and what 128 of them come to under a UUID-length profile
/// key (18.2 KiB of the 24 KiB household budget): re-derived whenever a bound moves.
const WORST_ENTRY_BYTES: usize = 144;
const WORST_PROFILE_BYTES: usize = 18_648;

/// Past the household budget the record holding the most entries gives up its OLDEST first, and
/// the writer's newest pick always stays.
#[test]
fn the_budget_evicts_the_largest_records_oldest() {
    let mut list = Vec::new();
    for n in 0..128 {
        assert!(put(&mut list, "heavy", VersionMemoryEntry::worst_case(n)));
    }
    for n in 1000..1060 {
        assert!(put(&mut list, "light", VersionMemoryEntry::worst_case(n)));
    }
    assert!(list_size(&list) <= BUDGET_BYTES, "the household stays inside its budget");
    let heavy = items_of(&list, "heavy");
    let light = items_of(&list, "light");
    assert_eq!(light.len(), 60, "the light viewer lost nothing");
    assert!(heavy.len() < 128, "the heavy viewer paid for it");
    let first_kept = heavy.first().expect("heavy keeps entries");
    assert_eq!(first_kept, &format!("{:016}", 128 - heavy.len()), "and lost its OLDEST, in order");
    assert_eq!(light.last().map(String::as_str), Some(format!("{:016}", 1059).as_str()), "the writer's newest stays");
}
