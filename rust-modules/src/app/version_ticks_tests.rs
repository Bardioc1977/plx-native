//! **The tick on "the version a bare Play takes"** (`docs/version-selection-spec.md` section 3): every capture
//! of a version list (a card's menu, the filmstrip episode's menu, the hero's Play Version panel)
//! ticks the profile's remembered version, else the Default (the row at WIRE index 0), else nothing.
//! The data layer cannot name the memory, so the application sets it when it captures the list.
use super::bridge::{card_menu_arg, episode_versions};
use plx_data::metadata::{Detail, Episode};
use plx_data::pms::{MediaVersion, PmsMovie};
use plx_plex::plex::session::prefs::VersionMemoryEntry;
use plx_plex::plex::{session, MediaId, ServerId};
use plx_screens::registry::{ContentPanel, ItemMenuKind};

const RK: &str = "movie-1";

fn row(id: i64, index: u32) -> MediaVersion {
    MediaVersion { id: MediaId(id), index, ..Default::default() }
}

/// Three versions with ids 10, 11, 12 at wire indices 0, 1, 2.
fn full() -> Vec<MediaVersion> {
    vec![row(10, 0), row(11, 1), row(12, 2)]
}

/// A Part-less `Media[0]` (id 10) dropped: two playable others, no row at wire index 0.
fn partless_default() -> Vec<MediaVersion> {
    vec![row(11, 1), row(12, 2)]
}

fn ticked(rows: &[MediaVersion]) -> Vec<i64> {
    rows.iter().filter(|v| v.plays).map(|v| v.id.0).collect()
}

/// The owner seated on a restored mirror, one registered server, and a remembered version if any.
struct Rig {
    sid: ServerId,
    _temp: session::TempSession,
    _serial: plx_base::testlock::Serial,
}

impl Rig {
    fn seated(remembered: Option<i64>) -> Rig {
        let serial = plx_base::testlock::serial();
        plx_plex::plex::reset_servers_for_test();
        let temp = session::TempSession::new("version-ticks");
        session::save(&session::Session { client_id: "cid-1".into(), account_token: "acct".into(), ..Default::default() });
        temp.seated("", "acct");
        plx_media::route::seed_memory_for_test::<VersionMemoryEntry>(&session::peek());
        let sid = plx_plex::plex::register_for_test("ticks", "10.0.0.9", 32400, "tok", "cid-ticks");
        if let Some(id) = remembered {
            assert!(plx_media::route::remember_version(sid, RK, Some(MediaId(id))), "setup: the pick is taken");
        }
        Rig { sid, _temp: temp, _serial: serial }
    }

    fn card(&self, versions: Vec<MediaVersion>) -> Vec<MediaVersion> {
        let item = PmsMovie { sid: self.sid, rk: RK.into(), versions, ..Default::default() };
        let arg = card_menu_arg(&item, false, false, plx_machine::machine::EntryId(1), None, None);
        match arg.kind {
            ItemMenuKind::Card { row, .. } => row.versions,
            _ => unreachable!("a card menu"),
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        plx_media::route::unseed_memory_for_test::<VersionMemoryEntry>();
        plx_plex::plex::reset_servers_for_test();
    }
}

/// The card menu's list: the remembered version, else the Default; a remembered id the item lost is
/// the Default (it plays); and with a Part-less `Media[0]` there is no Default row to tick.
#[test]
fn a_card_menu_ticks_the_version_a_bare_play_takes() {
    let rig = Rig::seated(None);
    assert_eq!(ticked(&rig.card(full())), [10], "nothing remembered: the Default");
    drop(rig);

    let rig = Rig::seated(Some(11));
    assert_eq!(ticked(&rig.card(full())), [11], "the remembered version, and only it");
    drop(rig);

    let rig = Rig::seated(Some(99));
    assert_eq!(ticked(&rig.card(full())), [10], "a remembered id the item lost: the Default plays");
    drop(rig);

    let rig = Rig::seated(None);
    assert_eq!(ticked(&rig.card(partless_default())), Vec::<i64>::new(), "no row is the Default: not the first row");
    drop(rig);

    let rig = Rig::seated(Some(12));
    assert_eq!(ticked(&rig.card(partless_default())), [12], "a remembered Media[2] is ticked");
}

/// An unseated boot (a dev token, `--guest`) remembers nothing, so a bare Play takes the Default.
#[test]
fn an_unseated_boot_ticks_the_default() {
    let _serial = plx_base::testlock::serial();
    plx_plex::plex::reset_servers_for_test();
    let _temp = session::TempSession::new("version-ticks-unseated");
    let sid = plx_plex::plex::register_for_test("ticks", "10.0.0.9", 32400, "tok", "cid-ticks");
    let item = PmsMovie { sid, rk: RK.into(), versions: full(), ..Default::default() };
    let arg = card_menu_arg(&item, false, false, plx_machine::machine::EntryId(1), None, None);
    let ItemMenuKind::Card { row, .. } = arg.kind else { unreachable!() };
    assert_eq!(ticked(&row.versions), [10]);
    plx_plex::plex::reset_servers_for_test();
}

/// A card with no choice has no list to tick (and the call costs nothing).
#[test]
fn a_card_without_versions_captures_none() {
    let rig = Rig::seated(Some(11));
    assert!(rig.card(Vec::new()).is_empty());
}

fn detail_with(sid: ServerId, movie_versions: Vec<MediaVersion>, episodes: Vec<Episode>, show: bool) -> Detail {
    Detail {
        sid,
        rk: RK.into(),
        is_show: show,
        kind: if show { "show".into() } else { "movie".into() },
        versions: movie_versions,
        episodes: episodes.into(),
        ..Default::default()
    }
}

fn episode(rk: &str, versions: Vec<MediaVersion>) -> Episode {
    Episode { rk: rk.into(), season: 1, versions, ..Default::default() }
}

/// The filmstrip episode's menu: the loaded show's episode of that key, ticked for ITS remembered
/// version (the memory is per episode).
#[test]
fn the_episode_menu_ticks_the_episodes_own_remembered_version() {
    let rig = Rig::seated(None);
    let mut store = plx_data::stores::metadata::MetadataStore::default();
    plx_data::metadata::set_current_for_test(
        store.state_mut(),
        Some(detail_with(rig.sid, Vec::new(), vec![episode("e1", full()), episode("e2", full())], true)),
    );
    assert!(plx_media::route::remember_version(rig.sid, "e2", Some(MediaId(12))));
    assert_eq!(ticked(&episode_versions(store.view(), rig.sid, "e1")), [10], "e1 has no pick: the Default");
    assert_eq!(ticked(&episode_versions(store.view(), rig.sid, "e2")), [12], "e2's own remembered version");
    assert!(episode_versions(store.view(), rig.sid, "elsewhere").is_empty(), "an episode that is not loaded");
    plx_data::metadata::set_current_for_test(store.state_mut(), None);
}

/// The hero's Play Version panel ticks the loaded movie's list, or on a show page the list of the
/// episode the hero's Play plays; any other panel leaves the store alone.
#[test]
fn the_hero_panel_is_ticked_before_it_is_presented() {
    let rig = Rig::seated(Some(11));
    let versions_panel = ContentPanel::Versions { anchor: [0; 4] };

    let mut store = plx_data::stores::metadata::MetadataStore::default();
    plx_data::metadata::set_current_for_test(store.state_mut(), Some(detail_with(rig.sid, full(), Vec::new(), false)));
    super::content::prepare_panel(&mut store, &ContentPanel::AltSources { anchor: [0; 4] });
    assert_eq!(ticked(&store.view().current().unwrap().versions), Vec::<i64>::new(), "another panel ticks nothing");
    super::content::prepare_panel(&mut store, &versions_panel);
    assert_eq!(ticked(&store.view().current().unwrap().versions), [11], "the movie's remembered version");

    // a show page: the hero plays its first episode (nothing resumes), and its list is the one ticked
    assert!(plx_media::route::remember_version(rig.sid, "e1", Some(MediaId(12))));
    plx_data::metadata::set_current_for_test(
        store.state_mut(),
        Some(detail_with(rig.sid, Vec::new(), vec![episode("e1", full()), episode("e2", full())], true)),
    );
    super::content::prepare_panel(&mut store, &versions_panel);
    let d = store.view().current().unwrap();
    assert_eq!(ticked(&d.episodes.get(0).unwrap().versions), [12], "the episode the hero's Play plays");
    assert_eq!(ticked(&d.episodes.get(1).unwrap().versions), Vec::<i64>::new(), "no other episode");
    plx_data::metadata::set_current_for_test(store.state_mut(), None);
}
