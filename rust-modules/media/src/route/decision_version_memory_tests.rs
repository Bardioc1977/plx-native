//! **The version choice through the memory door** (`docs/version-selection-spec.md` section 4.3):
//! what [`recalled_version`] and [`remember_version`] do on top of `prefs::{remember, recall}` —
//! the seat gate, the account stamp, the per-profile and per-server keys, the stale id, and the
//! forget on the default.
use super::test_support::*;
use super::*;
use plx_plex::plex::session::prefs::VersionMemoryEntry;
use plx_plex::plex::MediaId;
use plx_plex::plex::session::{self as session, Session};

fn signed_in(token: &str) -> Session {
    Session { client_id: "cid-1".into(), account_token: token.into(), ..Default::default() }
}

fn server(machine: &str) -> ServerId {
    plx_plex::plex::register_for_test(machine, "10.0.0.9", 32400, "tok", "cid-version-memory")
}

/// Leave the process as the next test expects it.
fn tidy() {
    plx_plex::plex::reset_servers_for_test();
    unseed_memory_for_test::<VersionMemoryEntry>();
}

/// A seated, seeded household: the owner is signed in as `acct` and the mirror is restored.
/// The lock is the FIRST of the pair: bind it first so it is dropped last, after the temp session
/// has put the process-global fixtures back.
fn seated(name: &str) -> (plx_base::testlock::Serial, session::TempSession) {
    let mut ps = crate::route::PlaybackSession::IDLE;
    let g = fresh_registry(&mut ps);
    let temp = session::TempSession::new(name);
    session::save(&signed_in("acct"));
    temp.seated("", "acct");
    assert_eq!(session::peek().client_id, "cid-1");
    seed_memory_for_test::<VersionMemoryEntry>(&session::peek());
    (g, temp)
}

/// **A pick is recalled at once and lands in the file, stamped.** The storage worker is parked, so
/// the file cannot have it yet when the mirror already answers.
#[test]
fn a_pick_is_recalled_before_its_write_lands_and_then_lands_stamped() {
    let (_g, _temp) = seated("version-memory-round-trip");
    let sid = server("machine-v1");
    assert_eq!(recalled_version(sid, "rk-1"), None, "nothing chosen yet");

    let parked = super::prefs_door_tests::Parked::new();
    assert!(remember_version(sid, "rk-1", Some(MediaId(7_000_002))));
    assert_eq!(recalled_version(sid, "rk-1"), Some(MediaId(7_000_002)));
    assert!(session::peek().prefs.version_memory.is_empty(), "setup: the write has not landed");
    parked.release();
    plx_base::storage_worker::drain_for_test();

    let stored = session::peek().prefs.version_memory.clone();
    assert_eq!(stored.len(), 1, "{stored:?}");
    assert_eq!((stored[0].user.as_str(), stored[0].account.as_str()), ("", plx_plex::plex::grant::account_key("acct").as_str()));
    assert_eq!(
        stored[0].items.iter().map(|e| (e.machine_id.as_str(), e.rating_key.as_str(), e.media_id)).collect::<Vec<_>>(),
        [("machine-v1", "rk-1", 7_000_002)]
    );
    tidy();
}

/// A later pick replaces the earlier one; `None` forgets it, in the mirror at once and in the file.
#[test]
fn a_repick_replaces_and_none_forgets() {
    let (_g, _temp) = seated("version-memory-repick");
    let sid = server("machine-v2");
    assert!(remember_version(sid, "rk-1", Some(MediaId(11))));
    assert!(remember_version(sid, "rk-1", Some(MediaId(22))));
    assert_eq!(recalled_version(sid, "rk-1"), Some(MediaId(22)));
    assert!(remember_version(sid, "rk-2", Some(MediaId(33))));

    let parked = super::prefs_door_tests::Parked::new();
    assert!(remember_version(sid, "rk-1", None), "the default: forget");
    assert_eq!(recalled_version(sid, "rk-1"), None, "forgotten at once");
    assert_eq!(recalled_version(sid, "rk-2"), Some(MediaId(33)), "another item's pick stays");
    parked.release();
    plx_base::storage_worker::drain_for_test();
    let items: Vec<_> = session::peek().prefs.version_memory.iter().flat_map(|r| r.items.iter()).map(|e| (e.rating_key.clone(), e.media_id)).collect();
    assert_eq!(items, [("rk-2".to_string(), 33)], "and the file forgot it too");
    tidy();
}

/// A non-positive id is no version: it records nothing and clears what was there.
#[test]
fn a_non_positive_id_is_not_a_pick() {
    let (_g, _temp) = seated("version-memory-nonpositive");
    let sid = server("machine-v3");
    assert!(remember_version(sid, "rk-1", Some(MediaId(44))));
    assert!(remember_version(sid, "rk-1", Some(MediaId(0))));
    assert_eq!(recalled_version(sid, "rk-1"), None);
    assert!(remember_version(sid, "rk-1", Some(MediaId(-1))));
    assert_eq!(recalled_version(sid, "rk-1"), None);
    tidy();
}

/// Two servers can share a `ratingKey`: the machine id is in the key, so a pick for one is not
/// found for the other, and an unregistered server finds and keeps nothing.
#[test]
fn two_servers_with_the_same_rating_key_do_not_collide() {
    let (_g, _temp) = seated("version-memory-two-servers");
    let (a, b) = (server("machine-a"), server("machine-b"));
    assert_ne!(a, b);
    assert!(remember_version(a, "42", Some(MediaId(111))));
    assert!(remember_version(b, "42", Some(MediaId(222))));
    assert_eq!(recalled_version(a, "42"), Some(MediaId(111)));
    assert_eq!(recalled_version(b, "42"), Some(MediaId(222)));
    assert!(remember_version(a, "42", None));
    assert_eq!(recalled_version(a, "42"), None);
    assert_eq!(recalled_version(b, "42"), Some(MediaId(222)), "the other server's pick is untouched");

    let nobody = unregistered_sid();
    assert!(!remember_version(nobody, "42", Some(MediaId(5))), "no server, no machine id, nothing to key on");
    assert_eq!(recalled_version(nobody, "42"), None);
    tidy();
}

/// A restore seeds the mirror from the record, so what a previous run remembered is found at the
/// next boot, and a profile's entry is not another profile's.
#[test]
fn a_restore_seeds_the_mirror_and_a_pick_is_the_seated_profiles_own() {
    let _g = plx_base::testlock::serial();
    let temp = session::TempSession::new("version-memory-restore");
    unseed_memory_for_test::<VersionMemoryEntry>();
    plx_plex::plex::reset_servers_for_test();
    let sid = server("machine-r");
    let mut record = signed_in("acct");
    let stamp = plx_plex::plex::grant::account_key("acct");
    record.prefs.version_memory = vec![
        plx_plex::plex::session::prefs::ProfileList {
            user: "kid".into(),
            account: stamp.clone(),
            items: vec![VersionMemoryEntry::new("machine-r", "rk", 555)],
            ..Default::default()
        },
        plx_plex::plex::session::prefs::ProfileList {
            user: String::new(),
            account: stamp,
            items: vec![VersionMemoryEntry::new("machine-r", "owners", 666)],
            ..Default::default()
        },
    ];

    temp.seated("kid", "acct");
    assert_eq!(recalled_version(sid, "rk"), None, "never restored: nothing is answered");
    restore_install_rows(&record);
    assert_eq!(recalled_version(sid, "rk"), Some(MediaId(555)));
    assert_eq!(recalled_version(sid, "other"), None, "another item has none");
    assert_eq!(recalled_version(sid, "owners"), None, "and not the owner's");
    temp.seated("", "acct");
    assert_eq!(recalled_version(sid, "owners"), Some(MediaId(666)), "seated as the owner, theirs is found");
    assert_eq!(recalled_version(sid, "rk"), None, "and the kid's is not");
    tidy();
}

/// A record stamped for another account reads as nothing.
#[test]
fn another_accounts_pick_reads_as_nothing() {
    let _g = plx_base::testlock::serial();
    let temp = session::TempSession::new("version-memory-foreign");
    unseed_memory_for_test::<VersionMemoryEntry>();
    plx_plex::plex::reset_servers_for_test();
    let sid = server("machine-f");
    let mut record = signed_in("acct");
    record.prefs.version_memory = vec![plx_plex::plex::session::prefs::ProfileList {
        user: String::new(),
        account: "ffffffffffffffff".into(),
        items: vec![VersionMemoryEntry::new("machine-f", "rk", 777)],
        ..Default::default()
    }];
    temp.seated("", "acct");
    restore_install_rows(&record);
    assert_eq!(recalled_version(sid, "rk"), None);
    tidy();
}

/// **A boot that seated nobody (a dev token, `--guest`, a replay) reads and writes nothing.** The
/// pick still has to work there: it rides the request (spec section 4.3), so
/// the door's refusal is the right answer, not a bug. Once the owner IS seated, the same call saves.
#[test]
fn an_unseated_boot_reads_and_writes_nothing() {
    let mut ps = crate::route::PlaybackSession::IDLE;
    let _g = fresh_registry(&mut ps);
    let temp = session::TempSession::new("version-memory-unseated");
    session::save(&signed_in("acct"));
    assert_eq!(session::current_seat(), session::Seat::Unseated, "setup: nobody is seated");
    seed_memory_for_test::<VersionMemoryEntry>(&session::peek());
    let sid = server("machine-u");

    assert!(!remember_version(sid, "rk", Some(MediaId(9))), "refused: no profile seated");
    assert_eq!(recalled_version(sid, "rk"), None);
    plx_base::storage_worker::drain_for_test();
    assert!(session::peek().prefs.version_memory.is_empty(), "and nothing reached the file");

    temp.seated("", "acct");
    assert_eq!(recalled_version(sid, "rk"), None, "the refused pick was not kept in the mirror");
    assert!(remember_version(sid, "rk", Some(MediaId(9))), "seated as the owner, the same call saves");
    assert_eq!(recalled_version(sid, "rk"), Some(MediaId(9)));
    plx_base::storage_worker::drain_for_test();
    assert_eq!(session::peek().prefs.version_memory.len(), 1);
    tidy();
}

/// A mirror nobody restored refuses a write and answers nothing: there is no valid session to
/// remember into, and the refusal leaves the file alone.
#[test]
fn a_mirror_nobody_restored_refuses_a_write_and_answers_nothing() {
    let _g = plx_base::testlock::serial();
    let temp = session::TempSession::new("version-memory-unseeded");
    session::save(&signed_in("acct"));
    temp.seated("", "acct");
    unseed_memory_for_test::<VersionMemoryEntry>();
    plx_plex::plex::reset_servers_for_test();
    let sid = server("machine-n");

    assert!(!remember_version(sid, "rk", Some(MediaId(5))));
    plx_base::storage_worker::drain_for_test();
    assert_eq!(recalled_version(sid, "rk"), None);
    assert!(session::peek().prefs.version_memory.is_empty(), "and nothing was queued");
    tidy();
}

// ---- recall once, settle at the landing ------------------------------------------------------------
//
// `request_play_inner` recalls the profile's pick into `request.version` ONCE, so both re-entries of
// the resolve (`pump_play`'s contract change and `retry_current_play`) re-send it; `pump_play` settles
// the memory at a playable landing of a fresh play (`docs/version-selection-spec.md` section 4.3). The Default is
// the `Media` at WIRE index 0: playing it forgets, whatever the list shows first.

#[cfg(feature = "devtriggers")]
mod settle {
    use super::super::version_tests::{media_index, world, World, DEFAULT_PART, PICKED, RK, TWO_VERSIONS};
    use super::*;
    use plx_plex::plex::session::prefs::VersionMemoryEntry;

    const MDE_DIRECT: &[u8] = MDE_DIRECTPLAY;
    const SECOND: MediaId = MediaId(3090);
    const DEFAULT: MediaId = MediaId(3086);

    fn secs(n: u64) -> std::time::Duration {
        std::time::Duration::from_secs(n)
    }

    /// An item whose versions are `(Media.id, Part.id)` in this order; a `None` Part is a version the
    /// server lists with no file to play.
    fn item_of(versions: &[(i64, Option<i64>)]) -> Vec<u8> {
        let media: Vec<_> = versions
            .iter()
            .map(|(id, part)| {
                let mut m = serde_json::json!({
                    "id": id, "videoCodec": "h264", "audioCodec": "aac", "width": 1920, "height": 1080,
                    "bitrate": 6_600, "container": "mkv", "duration": 6_000_000,
                });
                if let Some(part) = part {
                    m["Part"] = serde_json::json!([{ "id": part, "key": format!("/library/parts/{part}/1/file.mkv"),
                        "container": "mkv", "Stream": [
                            { "id": part * 10 + 1, "streamType": 1, "codec": "h264", "index": 0 },
                            { "id": part * 10 + 2, "streamType": 2, "codec": "aac", "channels": 2, "index": 1,
                              "languageCode": "eng", "default": true, "selected": true }] }]);
                }
                m
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({"MediaContainer": {"Metadata": [
            {"ratingKey": RK, "type": "episode", "duration": 6_000_000, "Media": media}]}}))
        .unwrap()
    }

    /// Three playable versions: ids 3086 (wire index 0, the Default), 3082 (1) and 3090 (2).
    fn three() -> Vec<u8> {
        item_of(&[(3086, Some(3134)), (3082, Some(3130)), (3090, Some(3140))])
    }

    /// The owner seated on a restored mirror; the mirror is put back to never-seeded when it drops.
    struct Seat {
        _temp: session::TempSession,
    }
    impl Seat {
        fn owner() -> Seat {
            let temp = session::TempSession::new("version-settle");
            session::save(&signed_in("acct"));
            temp.seated("", "acct");
            seed_memory_for_test::<VersionMemoryEntry>(&session::peek());
            Seat { _temp: temp }
        }
    }
    impl Drop for Seat {
        fn drop(&mut self) {
            let _ = wait_resolve_workers_for_test(secs(10));
            unseed_memory_for_test::<VersionMemoryEntry>();
        }
    }

    fn store() -> plx_data::stores::metadata::MetadataStore {
        plx_data::stores::metadata::MetadataStore::default()
    }

    /// Let the in-flight resolve land and pump it: what the player gets as its start point.
    fn land(w: &mut World, meta: &mut plx_data::stores::metadata::MetadataStore) -> Option<i64> {
        assert!(wait_resolve_workers_for_test(secs(10)));
        pump_play(&mut w.ps, meta)
    }

    fn stored(w: &World) -> Option<MediaId> {
        recalled_version(w.sid, RK)
    }

    fn indexes(lines: &[String]) -> Vec<Option<&str>> {
        lines.iter().map(|l| media_index(l)).collect()
    }

    // -- B1: the memory is read once, so the re-entries inherit it ----------------------------------

    /// **B1.** A remembered pick survives `pump_play`'s "desired contract changed in flight": the
    /// second resolve asks about the remembered version too, because the request carries it.
    #[test]
    fn a_remembered_version_survives_the_contract_changed_in_flight_re_resolve() {
        let mut w = world(TWO_VERSIONS, MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        assert_eq!(w.ps.request.as_ref().and_then(|r| r.version), Some(PICKED), "recalled once, onto the request");
        assert!(wait_resolve_workers_for_test(secs(10)));
        let first = w.pms.decisions_since(0, 1);
        assert!(!first.is_empty() && first.iter().all(|d| media_index(d) == Some("1")), "{first:?}");
        let seen = w.pms.requests().len();

        let edit = begin_user_contract_boundary();
        assert_eq!(pump_play(&mut w.ps, &mut meta), None, "the stale plan is not installed");
        assert!(wait_resolve_workers_for_test(secs(10)));
        let second = w.pms.decisions_since(seen, 1);
        assert!(!second.is_empty(), "the re-resolve asked the server");
        assert!(second.iter().all(|d| media_index(d) == Some("1")), "the second resolve is the memory's too: {second:?}");
        drop(edit);
        w.finish();
    }

    /// **B1.** A refused first plan, then a retry: the retry re-sends the request, which carries the
    /// recalled version. (A recall made only for a fresh play would have dropped it here.)
    #[test]
    fn a_retry_after_a_refused_first_plan_keeps_the_remembered_version() {
        let mut w = world(TWO_VERSIONS, MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        assert!(wait_resolve_workers_for_test(secs(10)));
        {
            let mut slot = PLAY_SLOT.lock().unwrap_or_else(|e| e.into_inner());
            slot.as_mut().expect("the first resolve landed").plan = Plan { direct_play_mode: DirectPlayMode::Auto, ..Default::default() };
        }
        assert_eq!(pump_play(&mut w.ps, &mut meta), None, "a plan with no URL starts nothing");
        assert_eq!(w.ps.request.as_ref().and_then(|r| r.version), Some(PICKED), "but the request still names it");
        let seen = w.pms.requests().len();

        assert!(retry_current_play(&mut w.ps, &mut meta, 0, None, crate::player::report::AttemptOrigin::UserRetry));
        assert!(wait_resolve_workers_for_test(secs(10)));
        let again = w.pms.decisions_since(seen, 1);
        assert!(!again.is_empty() && again.iter().all(|d| media_index(d) == Some("1")), "{again:?}");
        w.finish();
    }

    // -- explicit beats memory; the Default forgets --------------------------------------------------

    /// An explicit pick never consults the memory, and a landing of it replaces what was remembered.
    #[test]
    fn the_explicit_pick_wins_over_a_different_remembered_one() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(SECOND)));
        assert_eq!(w.ps.request.as_ref().and_then(|r| r.version), Some(SECOND));
        assert_eq!(land(&mut w, &mut meta).is_some(), true, "a playable landing");
        let seen = w.pms.decisions_since(0, 1);
        assert!(seen.iter().all(|d| media_index(d) == Some("2")), "{seen:?}");
        assert_eq!(cur_media_index(&w.ps), 2);
        assert_eq!(stored(&w), Some(SECOND), "the explicit pick replaced the remembered one");
        w.finish();
    }

    /// **"Index 0 forgets."** The row at wire index 0 is the Default; picking it explicitly overrides
    /// the memory and then forgets it, so a bare Play afterwards takes the Default again.
    #[test]
    fn a_pick_of_wire_index_0_forgets() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(DEFAULT)));
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(cur_media_index(&w.ps), 0);
        assert_eq!(stored(&w), None, "the Default needs no record, and the old pick is gone");
        // …so the next bare Play is the Default.
        reset_player_control_for_test(&w.ps);
        let seen = w.pms.requests().len();
        assert!(w.request_play(&mut meta, None));
        assert_eq!(w.ps.request.as_ref().map(|r| r.version), Some(None));
        assert!(wait_resolve_workers_for_test(secs(10)));
        let next = w.pms.decisions_since(seen, 1);
        assert!(!next.is_empty() && next.iter().all(|d| media_index(d) == Some("0")), "{next:?}");
        w.finish();
    }

    /// A pick of the Default when nothing was remembered leaves nothing, and writes nothing.
    #[test]
    fn a_pick_of_the_default_with_nothing_remembered_writes_nothing() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(DEFAULT)));
        assert!(land(&mut w, &mut meta).is_some());
        plx_base::storage_worker::drain_for_test();
        assert!(session::peek().prefs.version_memory.is_empty(), "the door was not called for an unchanged value");
        w.finish();
    }

    /// A pick of a non-default version is remembered at its landing, and a bare Play then takes it.
    #[test]
    fn a_landed_pick_is_remembered_and_the_next_bare_play_takes_it() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(PICKED)));
        assert_eq!(stored(&w), None, "nothing is written at the press");
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(stored(&w), Some(PICKED), "written at the landing");
        reset_player_control_for_test(&w.ps);
        let seen = w.pms.requests().len();
        assert!(w.request_play(&mut meta, None));
        assert_eq!(w.ps.request.as_ref().and_then(|r| r.version), Some(PICKED));
        assert!(wait_resolve_workers_for_test(secs(10)));
        let next = w.pms.decisions_since(seen, 1);
        assert!(!next.is_empty() && next.iter().all(|d| media_index(d) == Some("1")), "{next:?}");
        w.finish();
    }

    /// **The Default is the wire index, not the first row.** With a Part-less `Media[0]` and two
    /// playable others the list's first row is `Media[1]`; picking it plays `mediaIndex=1` and IS
    /// remembered (a "first row forgets" rule would forget it).
    #[test]
    fn with_a_partless_media_0_and_two_playable_others_a_pick_of_media_1_is_remembered() {
        let item = item_of(&[(3086, None), (3082, Some(3130)), (3090, Some(3140))]);
        let mut w = world(&item, MDE_DIRECT);
        let _seat = Seat::owner();
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(PICKED)));
        assert!(land(&mut w, &mut meta).is_some(), "the pick plays");
        assert_eq!(cur_media_index(&w.ps), 1, "wire index 1, though it is the list's first row");
        assert_eq!(stored(&w), Some(PICKED));
        w.finish();
    }

    /// The catalog row of an item whose Default (`Media[0]`) has no Part: its `part` is empty, as
    /// `Metadata::default_part` leaves it.
    fn partless_card(w: &World) -> plx_data::pms::PmsMovie {
        plx_data::pms::PmsMovie { sid: w.sid, rk: RK.to_owned(), title: "Movie".to_owned(), ..Default::default() }
    }

    /// **The card's Play Version row on an item whose Default cannot play.** The card has no Part
    /// key (`Media[0]` has none), but the row names a version that does: `request_play_movie` used
    /// to refuse on the empty card Part before the pick was read, so the one version the list
    /// offers did nothing. Found in the simulator against the mock's 940005 (S15).
    #[test]
    fn a_pick_plays_from_a_card_whose_default_has_no_part() {
        let item = item_of(&[(3086, None), (3082, Some(3130)), (3090, Some(3140))]);
        let mut w = world(&item, MDE_DIRECT);
        let _seat = Seat::owner();
        let mut meta = store();
        let card = partless_card(&w);
        assert!(request_play_movie(&mut w.ps, &mut meta, &card, "", Some(PICKED)), "the pick is not refused");
        assert!(land(&mut w, &mut meta).is_some(), "and it plays");
        assert_eq!(cur_media_index(&w.ps), 1);
        assert_eq!(stored(&w), Some(PICKED));
        w.finish();
    }

    /// A bare Play on that card takes the remembered version (the list ticks it), and is refused as
    /// before while nothing is remembered: the Default cannot play.
    #[test]
    fn a_bare_play_on_a_partless_default_takes_the_remembered_version_else_is_refused() {
        let item = item_of(&[(3086, None), (3082, Some(3130)), (3090, Some(3140))]);
        let mut w = world(&item, MDE_DIRECT);
        let _seat = Seat::owner();
        let mut meta = store();
        let card = partless_card(&w);
        assert!(!request_play_movie(&mut w.ps, &mut meta, &card, "", None), "nothing remembered: refused");
        assert!(w.pms.requests().is_empty(), "and nothing was asked of the server");
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        assert!(request_play_movie(&mut w.ps, &mut meta, &card, "", None), "the remembered pick plays");
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(cur_media_index(&w.ps), 1);
        assert_eq!(stored(&w), Some(PICKED));
        w.finish();
    }

    // -- stale ids ----------------------------------------------------------------------------------------

    /// A remembered id the item no longer lists plays the Default and is forgotten at that landing.
    #[test]
    fn a_vanished_remembered_id_is_forgotten_and_the_default_plays() {
        let mut w = world(TWO_VERSIONS, MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(MediaId(9999))));
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        assert!(land(&mut w, &mut meta).is_some(), "the default plays");
        assert_eq!(cur_media_index(&w.ps), 0);
        let seen = w.pms.decisions_since(0, 1);
        assert!(seen.iter().all(|d| media_index(d) == Some("0")), "{seen:?}");
        assert_eq!(stored(&w), None, "the dead id is dropped");
        w.finish();
    }

    /// The `Gone` arm: a version the item lists with no Part to play is as good as a lost one, so a
    /// pick of it plays the Default and forgets what was remembered.
    #[test]
    fn a_listed_but_partless_version_is_forgotten_like_a_lost_one() {
        let item = item_of(&[(3086, Some(3134)), (3082, None), (3090, Some(3140))]);
        let mut w = world(&item, MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(cur_media_index(&w.ps), 0, "the Part-less version cannot play");
        assert_eq!(stored(&w), None);
        w.finish();
    }

    /// **An item that could not be fetched says nothing about the version**, so a network blip must
    /// not erase a pick.
    #[test]
    fn a_failed_fetch_keeps_the_pick() {
        let mut w = world(b"not json at all", MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        let start = land(&mut w, &mut meta);
        assert!(start.is_some(), "the play still lands (the setup is not vacuous)");
        assert_eq!(stored(&w), Some(PICKED), "the unanswered fetch touched nothing");
        w.finish();
    }

    // -- what never recalls and never settles ----------------------------------------------------------------

    /// An Up Next successor is a fresh play of its own episode: it recalls that episode's pick.
    #[test]
    fn an_up_next_successor_recalls_its_own_episodes_pick() {
        let mut w = world(TWO_VERSIONS, MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        w.ps.cur_sid = w.sid;
        let mut meta = store();
        let next = UpNext { rk: RK.into(), part: DEFAULT_PART.into(), vcodec: "h264".into(), acodec: "aac".into(), ep_title: "Next".into(), ..Default::default() };
        assert!(request_play_up_next(&mut w.ps, &mut meta, next, ""));
        assert_eq!(w.ps.request.as_ref().and_then(|r| r.version), Some(PICKED));
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(cur_media_index(&w.ps), 1);
        w.finish();
    }

    /// Trailers, extras and previews never read the memory.
    #[test]
    fn trailers_extras_and_previews_never_recall() {
        let mut w = world(TWO_VERSIONS, MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        for ctx in [plx_data::metadata::TRAILER_CONTEXT, plx_data::metadata::EXTRA_CONTEXT] {
            assert!(request_play(&mut w.ps, &mut meta, w.sid, RK, DEFAULT_PART, "h264", "aac", "Extra", ctx, None));
            assert_eq!(w.ps.request.as_ref().map(|r| r.version), Some(None), "{ctx}: no recall");
            w.finish();
        }
        assert!(request_preview(&mut w.ps, &mut meta, w.sid, RK, DEFAULT_PART, "h264", "aac", "Trailer"));
        assert_eq!(w.ps.request.as_ref().map(|r| r.version), Some(None), "a preview recalls nothing");
        w.finish();
    }

    /// An extra, a trailer or a preview that nevertheless names a version never settles: the memory
    /// is the viewer's pick of the FEATURE.
    #[test]
    fn an_extra_a_trailer_and_a_preview_landing_never_settle() {
        for (ctx, preview) in [(plx_data::metadata::EXTRA_CONTEXT, false), (plx_data::metadata::TRAILER_CONTEXT, false), ("", true)] {
            let mut w = world(&three(), MDE_DIRECT);
            let _seat = Seat::owner();
            let mut meta = store();
            let request = PlaybackRequest {
                sid: w.sid,
                rk: RK.into(),
                part: DEFAULT_PART.into(),
                vcodec: "h264".into(),
                acodec: "aac".into(),
                title: "T".into(),
                ctx: ctx.into(),
                preview,
                version: Some(PICKED),
            };
            assert!(request_play_inner(&mut w.ps, &mut meta, request, None, None, false, crate::player::report::AttemptOrigin::Fresh));
            assert!(land(&mut w, &mut meta).is_some(), "{ctx:?}/{preview}: it lands");
            assert_eq!(cur_media_index(&w.ps), 1, "{ctx:?}/{preview}: the named version plays");
            assert_eq!(stored(&w), None, "{ctx:?}/{preview}: and is not remembered");
            w.finish();
        }
    }

    /// A retry's landing never settles: the pick was the first landing's business. A first landing
    /// that was refused leaves the retry as the only landing that plays; if it settled, the memory
    /// (changed behind the playback's back here) would be rewritten from a request the viewer did
    /// not make.
    #[test]
    fn a_retry_landing_never_settles() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        assert!(wait_resolve_workers_for_test(secs(10)));
        {
            let mut slot = PLAY_SLOT.lock().unwrap_or_else(|e| e.into_inner());
            slot.as_mut().expect("landed").plan.url.clear();
        }
        assert_eq!(pump_play(&mut w.ps, &mut meta), None, "the first landing is refused");
        // the memory changes behind the playback's back (another surface, a forget)
        assert!(remember_version(w.sid, RK, None));
        assert!(retry_current_play(&mut w.ps, &mut meta, 0, None, crate::player::report::AttemptOrigin::UserRetry));
        assert!(land(&mut w, &mut meta).is_some(), "the retry lands");
        assert_eq!(cur_media_index(&w.ps), 1, "and plays the version the request names");
        assert_eq!(stored(&w), None, "without writing it back");
        w.finish();
    }

    /// The contract re-resolve CONTINUES a fresh play, so an explicit pick still settles at its second
    /// landing.
    #[test]
    fn an_explicit_pick_still_settles_after_a_contract_changed_re_resolve() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(SECOND)));
        assert!(wait_resolve_workers_for_test(secs(10)));
        let edit = begin_user_contract_boundary();
        assert_eq!(pump_play(&mut w.ps, &mut meta), None);
        drop(edit);
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(stored(&w), Some(SECOND));
        w.finish();
    }

    /// A refused plan writes nothing: the old pick stands, whatever the refused request named.
    #[test]
    fn a_refused_plan_after_a_remembered_pick_leaves_the_old_pick_standing() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        assert!(remember_version(w.sid, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(SECOND)));
        assert!(wait_resolve_workers_for_test(secs(10)));
        {
            // the real plan, as the resolve built it, but with nothing to play: a refusal
            let mut slot = PLAY_SLOT.lock().unwrap_or_else(|e| e.into_inner());
            let landing = slot.as_mut().expect("landed");
            assert_eq!(landing.plan.version_settle, VersionSettle::Remember(SECOND), "setup: the plan would settle");
            landing.plan.url.clear();
        }
        assert_eq!(pump_play(&mut w.ps, &mut meta), None);
        assert_eq!(stored(&w), Some(PICKED), "a pick that cannot play is not remembered");
        w.finish();
    }

    // -- the door ------------------------------------------------------------------------------------------

    /// An unseated boot (a dev token, `--guest`) reads and writes nothing; the explicit pick still plays.
    #[test]
    fn an_unseated_boot_recalls_nothing_and_settles_nothing() {
        let mut w = world(&three(), MDE_DIRECT);
        let temp = session::TempSession::new("version-settle-unseated");
        session::save(&signed_in("acct"));
        assert_eq!(session::current_seat(), session::Seat::Unseated);
        seed_memory_for_test::<VersionMemoryEntry>(&session::peek());
        let mut meta = store();
        assert!(w.request_play(&mut meta, Some(PICKED)));
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(cur_media_index(&w.ps), 1, "the pick rides the request");
        plx_base::storage_worker::drain_for_test();
        assert!(session::peek().prefs.version_memory.is_empty(), "nothing reached the file");
        assert_eq!(stored(&w), None);
        w.finish();
        unseed_memory_for_test::<VersionMemoryEntry>();
        drop(temp);
    }

    /// Two servers can share a `ratingKey`: a pick remembered for one is not recalled for the other.
    #[test]
    fn two_servers_with_the_same_rating_key_do_not_collide() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        let elsewhere = plx_plex::plex::register_for_test("elsewhere", "127.0.0.1", 9, "tok", "cid-elsewhere");
        assert_ne!(elsewhere, w.sid);
        assert!(remember_version(elsewhere, RK, Some(PICKED)));
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        assert_eq!(w.ps.request.as_ref().map(|r| r.version), Some(None));
        assert!(land(&mut w, &mut meta).is_some());
        assert_eq!(cur_media_index(&w.ps), 0);
        assert_eq!(recalled_version(elsewhere, RK), Some(PICKED), "the other server's pick is untouched");
        w.finish();
    }

    /// **The golden.** A seated profile with no pick plays exactly as before: the request is
    /// unchanged, the server is asked about `mediaIndex=0` only, and nothing is written.
    #[test]
    fn a_seated_profile_with_no_pick_plays_byte_for_byte_as_before() {
        let mut w = world(&three(), MDE_DIRECT);
        let _seat = Seat::owner();
        let mut meta = store();
        assert!(w.request_play(&mut meta, None));
        let request = w.ps.request.clone().expect("the request");
        assert_eq!((request.version, request.preview, request.rk.as_str(), request.part.as_str()), (None, false, RK, DEFAULT_PART));
        assert!(land(&mut w, &mut meta).is_some());
        let decisions = w.pms.decisions_since(0, 1);
        assert!(indexes(&decisions).iter().all(|i| *i == Some("0")), "{decisions:?}");
        assert!(w.pms.requests().iter().all(|l| !l.contains("mediaIndex=1") && !l.contains("mediaIndex=2")));
        plx_base::storage_worker::drain_for_test();
        assert!(session::peek().prefs.version_memory.is_empty());
        w.finish();
    }
}
