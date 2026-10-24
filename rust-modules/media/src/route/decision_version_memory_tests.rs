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
