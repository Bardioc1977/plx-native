//! Play Version: a picked `Media[]` entry reaches every reader of the play path — the MDE ask,
//! the transcode decision and start URL, the direct-play part, the stream selection and the track
//! store — and none of them takes `Media[0]`.

use super::*;
#[allow(unused_imports)]
use super::test_support::*;

/// An episode with two versions, listed default-first: a 1080p h264/aac file and a 4K hevc/eac3
/// one, each with its own part and its own audio stream ids.
pub(super) const TWO_VERSIONS: &[u8] = br#"{"MediaContainer":{"Metadata":[{"ratingKey":"rk-v","type":"episode","Media":[
  {"id":3086,"bitrate":2372,"width":1920,"height":1080,"videoResolution":"1080","videoCodec":"h264","audioCodec":"aac","container":"mkv",
   "Part":[{"id":3134,"key":"/library/parts/3134/1/file.mkv","container":"mkv","Stream":[
     {"id":31341,"streamType":1,"codec":"h264","index":0},
     {"id":31342,"streamType":2,"codec":"aac","channels":2,"index":1,"languageCode":"eng","default":true,"selected":true}]}]},
  {"id":3082,"bitrate":14663,"width":3840,"height":2160,"videoResolution":"4k","videoCodec":"hevc","audioCodec":"eac3","container":"mkv",
   "Part":[{"id":3130,"key":"/library/parts/3130/1/file.mkv","container":"mkv","Stream":[
     {"id":31301,"streamType":1,"codec":"hevc","index":0},
     {"id":31302,"streamType":2,"codec":"eac3","channels":6,"index":1,"languageCode":"eng","default":true,"selected":true}]}]}]}]}}"#;

pub(super) const RK: &str = "rk-v";
/// The Part key a menu hands the play: version 0's, whatever version the viewer picked.
pub(super) const DEFAULT_PART: &str = "/library/parts/3134/1/file.mkv";
pub(super) const PICKED: plx_plex::plex::MediaId = plx_plex::plex::MediaId(3082);

pub(super) struct PmsState {
    requests: Vec<String>,
    mde: &'static [u8],
    pub(super) item: Vec<u8>,
}

/// A loopback PMS that serves the fixture item for `rk-v`, `mde` for every `/decision`, every
/// `PUT` accepted and an empty container for the rest, and records every request line until it
/// is dropped.
pub(super) struct VersionPms {
    port: i32,
    pub(super) state: std::sync::Arc<std::sync::Mutex<PmsState>>,
    _alive: std::sync::mpsc::Sender<()>,
}

impl VersionPms {
    fn start(item: &[u8], mde: &'static [u8]) -> VersionPms {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port() as i32;
        listener.set_nonblocking(true).unwrap();
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let state = std::sync::Arc::new(std::sync::Mutex::new(PmsState { requests: Vec::new(), mde, item: item.to_vec() }));
        let served = state.clone();
        std::thread::spawn(move || loop {
            match plx_base::testnet::accept(&listener) {
                Ok((mut socket, _)) => {
                    let line = drain_http(&mut socket);
                    let (mde, item) = {
                        let mut s = served.lock().unwrap_or_else(|e| e.into_inner());
                        s.requests.push(line.clone());
                        (s.mde, s.item.clone())
                    };
                    if line.starts_with("PUT ") {
                        write_status(&mut socket, 200);
                    } else if line.starts_with("GET /library/metadata/rk-v") {
                        write_json(&mut socket, &item);
                    } else if line.contains("/decision?") {
                        write_json(&mut socket, mde);
                    } else {
                        write_json(&mut socket, EMPTY_MC);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if !matches!(stopped.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(e) => panic!("fixture accept: {e}"),
            }
        });
        VersionPms { port, state, _alive: stop }
    }

    pub(super) fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).requests.clone()
    }

    /// The `/decision` request lines the server saw from request number `from` on, waiting up to
    /// ten seconds for `n` of them.
    pub(super) fn decisions_since(&self, from: usize, n: usize) -> Vec<String> {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let seen: Vec<String> =
                self.requests().into_iter().skip(from).filter(|l| l.contains("/decision?")).collect();
            if seen.len() >= n || std::time::Instant::now() > end {
                return seen;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

/// One registered loopback server and an idle session, torn down (workers drained, registry
/// emptied) when it goes out of scope.
pub(super) struct World {
    pub(super) ps: crate::route::PlaybackSession,
    pub(super) pms: VersionPms,
    pub(super) sid: ServerId,
    _serial: plx_base::testlock::Serial,
}

pub(super) fn world(item: &[u8], mde: &'static [u8]) -> World {
    let mut ps = crate::route::PlaybackSession::IDLE;
    let serial = fresh_registry(&mut ps);
    let pms = VersionPms::start(item, mde);
    let sid = plx_plex::plex::register_for_test("versions", "127.0.0.1", pms.port, "token", "versions-client");
    World { ps, pms, sid, _serial: serial }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = wait_resolve_workers_for_test(std::time::Duration::from_secs(10));
        plx_plex::plex::reset_servers_for_test();
        reset_player_control_for_test(&self.ps);
        reset_session(&mut self.ps);
    }
}

impl World {
    /// The environment a play of `RK` resolves under, the pick being `version`.
    pub(super) fn env(&self, version: Option<plx_plex::plex::MediaId>) -> ResolveEnv {
        let mut env = ResolveEnv::snapshot(
            &self.ps,
            plx_data::stores::metadata::MetadataStore::default().view(),
            self.sid,
            RK,
        );
        env.version = version;
        env
    }

    /// `build_stream` for the item, the arguments being what a menu holds (version 0's).
    pub(super) fn resolve(&self, env: &ResolveEnv) -> Plan {
        build_stream(&plx_base::task::OffFrame::for_test(), RK, DEFAULT_PART, "h264", "aac", env)
    }

    /// The request a menu's play makes, through the real entry point.
    pub(super) fn request_play(&mut self, meta: &mut plx_data::stores::metadata::MetadataStore, version: Option<plx_plex::plex::MediaId>) -> bool {
        request_play(&mut self.ps, meta, self.sid, RK, DEFAULT_PART, "h264", "aac", "Episode", "", version)
    }

    pub(super) fn finish(&mut self) {
        cancel_play(&mut self.ps);
        assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)), "a resolve worker is still running");
    }
}

/// Resolve `rk-v` as the request a menu's play makes: the default's part and codecs (what the
/// listing row carries), plus the version the viewer picked.
fn resolve(mde: &'static [u8], version: Option<plx_plex::plex::MediaId>) -> (Plan, Vec<String>) {
    let w = world(TWO_VERSIONS, mde);
    let plan = w.resolve(&w.env(version));
    let requests = w.pms.requests();
    (plan, requests)
}

#[test]
#[cfg(feature = "devtriggers")]
fn a_picked_version_direct_plays_its_own_part_after_asking_about_it() {
    let (plan, requests) = resolve(MDE_DIRECTPLAY, Some(plx_plex::plex::MediaId(3082)));
    let mde = requests.iter().find(|l| l.contains("/decision?")).unwrap_or_else(|| panic!("{requests:?}"));
    assert_eq!(query_param(mde, "mediaIndex"), Some("1"), "the MDE asks about the picked version: {mde}");
    assert_eq!(query_param(mde, "audioStreamID"), Some("31302"), "with that version's audio: {mde}");
    assert!(plan.url.contains("/library/parts/3130/"), "the picked version's part plays: {}", plan.url);
    assert_eq!((plan.part_id, plan.media_index), (3130, 1));
    assert_eq!((plan.src_vcodec.as_str(), plan.src_acodec.as_str()), ("hevc", "eac3"));
    let playing = plan.playing.as_ref().expect("track store");
    assert_eq!((playing.width, playing.audio[0].id), (3840, 31302), "the track store is the picked version's");
    for put in requests.iter().filter(|l| l.starts_with("PUT /library/parts/")) {
        assert!(put.starts_with("PUT /library/parts/3130"), "a selection names the picked part: {put}");
    }
}

#[test]
#[cfg(feature = "devtriggers")]
fn a_picked_version_transcodes_from_its_own_index() {
    let (plan, requests) = resolve(MDE_TRANSCODE_VIDEO, Some(plx_plex::plex::MediaId(3082)));
    let decisions: Vec<&String> = requests.iter().filter(|l| l.contains("/decision?")).collect();
    assert!(decisions.len() >= 2, "an MDE ask and a transcode decision: {requests:?}");
    for line in &decisions {
        assert_eq!(query_param(line, "mediaIndex"), Some("1"), "{line}");
    }
    assert!(plan.url.contains("start.mkv"), "{}", plan.url);
    assert!(plan.url.contains("mediaIndex=1&"), "the start URL names the picked version: {}", plan.url);
    assert_eq!(plan.part_id, 3130);
    for put in requests.iter().filter(|l| l.starts_with("PUT /library/parts/")) {
        assert!(put.starts_with("PUT /library/parts/3130"), "a selection names the picked part: {put}");
    }
}

#[test]
#[cfg(feature = "devtriggers")]
fn a_version_the_item_no_longer_lists_plays_the_default() {
    let (plan, requests) = resolve(MDE_DIRECTPLAY, Some(plx_plex::plex::MediaId(9)));
    let mde = requests.iter().find(|l| l.contains("/decision?")).unwrap_or_else(|| panic!("{requests:?}"));
    assert_eq!(query_param(mde, "mediaIndex"), Some("0"), "{mde}");
    assert!(plan.url.contains("/library/parts/3134/"), "{}", plan.url);
    assert_eq!((plan.part_id, plan.media_index), (3134, 0));
    assert_eq!(plan.playing.as_ref().map(|p| p.audio[0].id), Some(31342));
}

#[test]
#[cfg(feature = "devtriggers")]
fn no_pick_is_the_default_version_exactly_as_before() {
    let (plan, requests) = resolve(MDE_DIRECTPLAY, None);
    let mde = requests.iter().find(|l| l.contains("/decision?")).unwrap_or_else(|| panic!("{requests:?}"));
    assert_eq!(query_param(mde, "mediaIndex"), Some("0"), "{mde}");
    assert_eq!((plan.part_id, plan.media_index), (3134, 0));
}

/// A retry or quality change resolves the same item again from the stored request, so the pick
/// rides it; the session installs the landed version for every later encoder of the playback.
#[test]
fn the_picked_version_rides_the_request_and_the_landing() {
    let mut ps = crate::route::PlaybackSession::IDLE;
    let _g = fresh_registry(&mut ps);
    let mut meta = plx_data::stores::metadata::MetadataStore::default();
    assert!(request_play(&mut ps, &mut meta, ServerId::from_raw(0), "rk-v", "/library/parts/3134/1/file.mkv",
        "h264", "aac", "Episode", "", Some(plx_plex::plex::MediaId(3082))));
    assert_eq!(ps.request.as_ref().and_then(|r| r.version), Some(plx_plex::plex::MediaId(3082)));
    let env = request_env(&ps, meta.view(), ps.request.as_ref().unwrap(), None);
    assert_eq!(env.version, Some(plx_plex::plex::MediaId(3082)));
    assert!(env.cached_item.is_none(), "the default's streams never stand in for the pick");
    cancel_play(&mut ps);
    test_support::apply_plan(&mut ps, Plan { media_index: 1, ..Default::default() }, "rk-v");
    assert_eq!(cur_media_index(&ps), 1);
}

// ---- the pick survives every re-entry of the resolve ---------------------------------------------

/// The `mediaIndex` an MDE ask or transcode decision line carries.
pub(super) fn media_index(line: &str) -> Option<&str> {
    query_param(line, "mediaIndex")
}

/// `pump_play`'s "desired contract changed in flight" discards the landed plan and resolves again
/// from `ps.request`. The pick rides that request, so the second resolve asks about the same
/// version. (Characterisation: it holds at `origin/main`; the delta's memory must keep it holding.)
#[test]
#[cfg(feature = "devtriggers")]
fn a_pick_survives_the_contract_changed_in_flight_re_resolve() {
    let mut w = world(TWO_VERSIONS, MDE_DIRECTPLAY);
    let mut meta = plx_data::stores::metadata::MetadataStore::default();
    assert!(w.request_play(&mut meta, Some(PICKED)));
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    let first = w.pms.decisions_since(0, 1);
    assert!(!first.is_empty() && first.iter().all(|d| media_index(d) == Some("1")), "{first:?}");
    let seen = w.pms.requests().len();

    // the desired contract moves while the plan sits in the mailbox
    let edit = begin_user_contract_boundary();
    assert_eq!(pump_play(&mut w.ps, &mut meta), None, "the stale plan is not installed");
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    let second = w.pms.decisions_since(seen, 1);
    assert!(!second.is_empty(), "the re-resolve asked the server");
    assert!(second.iter().all(|d| media_index(d) == Some("1")), "the second resolve is the pick's too: {second:?}");
    assert_eq!(w.ps.request.as_ref().and_then(|r| r.version), Some(PICKED));
    drop(edit);
    w.finish();
}

/// A refused first plan lands as a default `Plan` (no URL, media index 0). The retry that follows
/// re-sends `ps.request`, which still names the version, so it resolves the pick again.
#[test]
#[cfg(feature = "devtriggers")]
fn a_retry_after_a_refused_first_plan_keeps_the_pick() {
    let mut w = world(TWO_VERSIONS, MDE_DIRECTPLAY);
    let mut meta = plx_data::stores::metadata::MetadataStore::default();
    assert!(w.request_play(&mut meta, Some(PICKED)));
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));

    // the landing is a refused (or panicked) resolve: a default plan with no URL
    {
        let mut slot = PLAY_SLOT.lock().unwrap_or_else(|e| e.into_inner());
        let landing = slot.as_mut().expect("the first resolve landed");
        landing.plan = Plan { direct_play_mode: DirectPlayMode::Auto, ..Default::default() };
    }
    assert_eq!(pump_play(&mut w.ps, &mut meta), None, "a plan with no URL starts nothing");
    assert_eq!(cur_media_index(&w.ps), 0, "the refused landing carries no version");
    assert_eq!(w.ps.request.as_ref().and_then(|r| r.version), Some(PICKED), "but the request still names it");
    let seen = w.pms.requests().len();

    assert!(retry_current_play(&mut w.ps, &mut meta, 0, None, crate::player::report::AttemptOrigin::UserRetry));
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    let again = w.pms.decisions_since(seen, 1);
    assert!(!again.is_empty() && again.iter().all(|d| media_index(d) == Some("1")), "{again:?}");
    w.finish();
}

// ---- the file that plays is the file every later reader describes ---------------------------------

/// A retranscode of a picked version names the PICKED file's codecs, also in the fallback it takes
/// when the server's `/decision` cannot be read: `Plan.src_vcodec/src_acodec` are the played
/// file's, `apply_plan` installs them, and the claim inputs copy them from the session.
#[test]
#[cfg(feature = "devtriggers")]
fn a_retranscode_uses_the_picked_files_codecs_in_every_fallback() {
    let mut w = world(TWO_VERSIONS, MDE_DIRECTPLAY);
    let plan = w.resolve(&w.env(Some(PICKED)));
    assert_eq!((plan.src_vcodec.as_str(), plan.src_acodec.as_str()), ("hevc", "eac3"), "the plan names the played file");
    test_support::apply_plan(&mut w.ps, plan, RK);
    assert_eq!((w.ps.src_vcodec.as_str(), w.ps.src_acodec.as_str()), ("hevc", "eac3"), "the session says it too");
    assert_eq!(cur_media_index(&w.ps), 1);

    let ticket = worker_ticket();
    let inputs = prepare_retranscode_inputs(&w.ps, &ticket, 0).expect("a transcode-capable route");
    assert_eq!((inputs.src_vcodec.as_str(), inputs.src_acodec.as_str()), ("hevc", "eac3"));
    assert_eq!(inputs.media_index, 1, "and the spec it builds names version 1");
    let remux = enhanced_remux_contract(plx_plex::plex::AudioEnhancements::NONE, false);
    let (video, audio) = retranscode_fallback_codecs(&inputs, &remux);
    assert_eq!(video, "hevc", "the fallback copies the picked file's video");
    assert_eq!(audio, remux_output_acodec(false, inputs.carried_audio.as_ref(), "eac3"));
    assert_ne!(audio, "aac", "never version 0's audio");
}

// ---- track memory is matched by descriptor, so it follows an episode across versions ----------------

/// An episode with two versions whose audio tracks are the SAME two tracks (English and Japanese
/// E-AC3 5.1) under different `Stream.id`s.
fn episode_with_two_versions() -> Vec<u8> {
    let media = |id: i64, part: i64| {
        serde_json::json!({
            "id": id, "videoCodec": "h264", "audioCodec": "eac3", "width": 1920, "height": 1080,
            "bitrate": 8_000, "duration": 3_600_000, "container": "mkv",
            "Part": [{ "id": part, "key": format!("/library/parts/{part}/1/file.mkv"), "container": "mkv", "Stream": [
                { "id": part * 10 + 1, "streamType": 1, "codec": "h264", "frameRate": 24.0, "bitrate": 6_500 },
                { "id": part * 10 + 2, "streamType": 2, "index": 1, "codec": "eac3", "channels": 6,
                  "languageCode": "eng", "selected": 1 },
                { "id": part * 10 + 3, "streamType": 2, "index": 2, "codec": "eac3", "channels": 6,
                  "languageCode": "jpn" },
            ] }],
        })
    };
    serde_json::to_vec(&serde_json::json!({"MediaContainer": {"Metadata": [{
        "ratingKey": RK, "type": "episode", "grandparentRatingKey": "4242", "duration": 3_600_000,
        "Media": [media(301, 37_001), media(302, 37_002)],
    }]}}))
    .unwrap()
}

/// A remembered Japanese audio pick for the show matches the Japanese track of WHICHEVER version
/// plays: the memory is keyed on the track's descriptor (language, codec, channels), never on a
/// `Stream.id`, which differs between the versions of one item.
#[test]
#[cfg(feature = "devtriggers")]
fn a_remembered_audio_track_matches_in_every_version_of_the_episode() {
    use plx_plex::plex::session::prefs::{AudioMemo, TrackMemoryEntry};
    let w = world(&episode_with_two_versions(), MDE_DIRECTPLAY);
    let memory = vec![TrackMemoryEntry::new(
        "versions",
        "4242",
        Some(AudioMemo { lang: "jpn".into(), codec: "eac3".into(), channels: 6, ..Default::default() }),
        None,
    )];
    let audio_of = |version: Option<plx_plex::plex::MediaId>| {
        let mut env = w.env(version);
        env.memory = memory.clone();
        env.fresh_play = true;
        let plan = build_stream(&plx_base::task::OffFrame::for_test(), RK, "/library/parts/37001/1/file.mkv", "h264", "eac3", &env);
        (plan.audio.as_ref().map(|a| a.sid), plan.part_id)
    };
    assert_eq!(audio_of(None), (Some(370_013), 37_001), "version 0: its own Japanese track");
    assert_eq!(
        audio_of(Some(plx_plex::plex::MediaId(302))),
        (Some(370_023), 37_002),
        "version 1: ITS Japanese track, same memory"
    );
}

// ---- the quality ceiling is judged against the rate of the file that plays ------------------------

/// An episode with two 1080p h264/aac versions whose video-stream and whole-file rates the caller
/// chooses: version 0 (id 3086) 6000 kbps video in a 6600 kbps file, version 1 (id 3082) as given.
fn rated_versions(video1: i64, whole1: i64) -> Vec<u8> {
    let media = |id: i64, part: i64, video: i64, whole: i64| {
        serde_json::json!({
            "id": id, "videoCodec": "h264", "audioCodec": "aac", "width": 1920, "height": 1080,
            "videoResolution": "1080", "bitrate": whole, "container": "mkv",
            "Part": [{ "id": part, "key": format!("/library/parts/{part}/1/file.mkv"), "container": "mkv", "Stream": [
                { "id": part * 10 + 1, "streamType": 1, "codec": "h264", "index": 0, "bitrate": video },
                { "id": part * 10 + 2, "streamType": 2, "codec": "aac", "channels": 2, "index": 1,
                  "languageCode": "eng", "default": true, "selected": true },
            ] }],
        })
    };
    serde_json::to_vec(&serde_json::json!({"MediaContainer": {"Metadata": [{
        "ratingKey": RK, "type": "episode",
        "Media": [media(3086, 3134, 6_000, 6_600), media(3082, 3130, video1, whole1)],
    }]}}))
    .unwrap()
}

/// Resolve `rk-v` under `quality` with the pick `version`, MDE answering direct play.
fn resolve_rated(item: &[u8], quality: Quality, version: Option<plx_plex::plex::MediaId>, src_kbps: i64) -> Plan {
    let w = world(item, MDE_DIRECTPLAY);
    let mut env = w.env(version);
    env.quality = quality;
    env.src_kbps = src_kbps;
    w.resolve(&env)
}

/// **U1.** A fixed rung (`maxVideoBitrate` bounds the VIDEO lane) is judged against the rate of the
/// file that plays: the picked version's own video stream, not 0. A 6000 kbps video in an 8500 kbps
/// file fits the 8000 kbps rung (the whole-file figure would not), so it direct-plays its own part.
#[test]
#[cfg(feature = "devtriggers")]
fn a_picked_version_is_measured_from_its_own_video_stream_under_a_quality_rung() {
    // `src_kbps` 0: what `request_env` hands a picked version today (the default's rate is not its own)
    let plan = resolve_rated(&rated_versions(6_000, 8_500), Quality::P1080, Some(PICKED), 0);
    assert_eq!(plan.src_measure.0, 6_000, "the ceiling is judged against the picked file's video rate");
    assert!(plan.url.contains("/library/parts/3130/"), "a version under the rung direct-plays: {}", plan.url);
    assert!(!plan.url.contains("start.mkv"), "and is not re-encoded: {}", plan.url);
    assert_eq!((plan.part_id, plan.media_index), (3130, 1));
}

/// **U1.** Above the rung the same measurement denies direct play and remux: the measured rate is
/// 9000, so the plan re-encodes (and says what it measured, for every later quality change).
#[test]
#[cfg(feature = "devtriggers")]
fn a_picked_version_above_the_rung_is_re_encoded_and_remembers_what_it_measured() {
    let plan = resolve_rated(&rated_versions(9_000, 9_500), Quality::P1080, Some(PICKED), 0);
    assert_eq!(plan.src_measure.0, 9_000);
    assert!(plan.url.contains("start.mkv"), "{}", plan.url);
}

/// **U1.** PMS omits a stream's own rate often enough that the whole-file figure stays the fallback
/// (the conservative direction, `plx_plex::plex::Ceiling::admits`), for a picked version as for the
/// default.
#[test]
#[cfg(feature = "devtriggers")]
fn a_picked_version_without_a_video_stream_rate_falls_back_to_the_whole_file() {
    let plan = resolve_rated(&rated_versions(0, 7_500), Quality::P1080, Some(PICKED), 0);
    assert_eq!(plan.src_measure.0, 7_500);
    assert!(plan.url.contains("/library/parts/3130/"), "{}", plan.url);
    let over = resolve_rated(&rated_versions(0, 8_500), Quality::P1080, Some(PICKED), 0);
    assert_eq!(over.src_measure.0, 8_500);
    assert!(over.url.contains("start.mkv"), "{}", over.url);
}

/// **U1.** The row at wire index 0 is the Default: picking it behaves like a default play of the
/// same item (same route, same part, same measurement), the rate of a default play being what the
/// loaded detail hands the environment (`ResolveEnv::src_kbps`, version 0's video rate).
#[test]
#[cfg(feature = "devtriggers")]
fn picking_the_default_row_plays_like_a_default_play_under_a_rung() {
    let item = rated_versions(9_000, 9_500);
    let default = resolve_rated(&item, Quality::P1080, None, 6_000);
    let picked = resolve_rated(&item, Quality::P1080, Some(plx_plex::plex::MediaId(3086)), 0);
    let route = |p: &Plan| {
        let path = p.url.split('?').next().unwrap_or_default();
        path[path.find("/video").or_else(|| path.find("/library")).unwrap_or(0)..].to_owned()
    };
    assert_eq!(route(&picked), route(&default), "the same route to the same part");
    assert_eq!(picked.src_measure, default.src_measure);
    assert_eq!((picked.part_id, picked.media_index), (default.part_id, default.media_index));
    assert_eq!(default.src_measure.0, 6_000);
}

// ---- the landing names the Part that plays ------------------------------------------------------------

/// **U2.** `Plan.part` is the Part key that plays (the picked version's own, the argument for a
/// default play and for a pick the item has lost) and the landing installs it as `cur_part`, beside
/// `cur_part_id`; the request keeps what the caller asked for.
#[test]
#[cfg(feature = "devtriggers")]
fn a_landing_names_the_part_that_plays_and_leaves_the_request_alone() {
    const PICKED_PART: &str = "/library/parts/3130/1/file.mkv";
    let mut w = world(TWO_VERSIONS, MDE_DIRECTPLAY);
    let mut meta = plx_data::stores::metadata::MetadataStore::default();
    assert!(w.request_play(&mut meta, Some(PICKED)));
    w.finish();

    for (pick, part, id) in [
        (Some(PICKED), PICKED_PART, 3130),
        (None, DEFAULT_PART, 3134),
        (Some(plx_plex::plex::MediaId(9)), DEFAULT_PART, 3134),
    ] {
        let plan = w.resolve(&w.env(pick));
        assert_eq!(plan.part, part, "{pick:?}");
        assert_eq!(plan.part_id, id, "{pick:?}");
        test_support::apply_plan(&mut w.ps, plan, RK);
        assert_eq!(w.ps.cur_part, part, "{pick:?}: the session names the Part that plays");
        assert_eq!(cur_part_id(&w.ps), id);
        assert_eq!(w.ps.request.as_ref().map(|r| r.part.as_str()), Some(DEFAULT_PART), "the request is what was asked");
        reset_session(&mut w.ps);
        w.ps.request = Some(PlaybackRequest {
            sid: w.sid,
            rk: RK.into(),
            part: DEFAULT_PART.into(),
            vcodec: "h264".into(),
            acodec: "aac".into(),
            title: String::new(),
            ctx: String::new(),
            preview: false,
            version: Some(PICKED),
        });
    }
}

// ---- a resume point is judged against the length of the version that plays ------------------------

/// An episode whose version 0 (id 3086) runs 100 minutes and whose version 1 (id 3082) runs 90.
fn two_lengths() -> Vec<u8> {
    let media = |id: i64, part: i64, ms: i64| {
        serde_json::json!({
            "id": id, "videoCodec": "h264", "audioCodec": "aac", "width": 1920, "height": 1080,
            "bitrate": 6_600, "container": "mkv", "duration": ms,
            "Part": [{ "id": part, "key": format!("/library/parts/{part}/1/file.mkv"), "container": "mkv", "Stream": [
                { "id": part * 10 + 1, "streamType": 1, "codec": "h264", "index": 0 },
                { "id": part * 10 + 2, "streamType": 2, "codec": "aac", "channels": 2, "index": 1,
                  "languageCode": "eng", "default": true, "selected": true },
            ] }],
        })
    };
    serde_json::to_vec(&serde_json::json!({"MediaContainer": {"Metadata": [{
        "ratingKey": RK, "type": "episode", "duration": 6_000_000,
        "Media": [media(3086, 3134, 6_000_000), media(3082, 3130, 5_400_000)],
    }]}}))
    .unwrap()
}

/// What `pump_play` hands the player as the start point for a play of `version` armed with a resume
/// point of `resume_min` minutes, and the point a retry would reopen at.
fn start_of(version: Option<plx_plex::plex::MediaId>, resume_min: i64) -> (Option<i64>, i64) {
    let mut w = world(&two_lengths(), MDE_DIRECTPLAY);
    let mut meta = plx_data::stores::metadata::MetadataStore::default();
    assert!(w.request_play(&mut meta, version));
    assert!(arm_play_resume(&mut w.ps, resume_min * 60 * 1_000_000_000));
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    let start = pump_play(&mut w.ps, &mut meta);
    let retry = unpresented_resume_ns(&w.ps);
    w.finish();
    (start, retry)
}

/// **U5.** The resume point a card or page carries is judged against version 0's length. Version 1
/// is 10 minutes shorter, so a 95-minute point lies past its end: the play starts over, and a retry
/// of that play must not reopen on the credits either.
#[test]
#[cfg(feature = "devtriggers")]
fn a_resume_point_past_the_end_of_the_picked_version_starts_over() {
    let min = |m: i64| m * 60 * 1_000_000_000;
    assert_eq!(start_of(Some(PICKED), 95), (Some(0), 0), "past the picked version's end: from the start");
    assert_eq!(start_of(Some(PICKED), 60), (Some(min(60)), min(60)), "inside it: unchanged");
    assert_eq!(start_of(None, 95), (Some(min(95)), min(95)), "a default play is untouched (judged by the page)");
    assert_eq!(
        start_of(Some(plx_plex::plex::MediaId(9)), 95),
        (Some(min(95)), min(95)),
        "a pick the item lost plays the default and keeps the point"
    );
}

// ---- a /decision body about another version is never judged as the pick's ---------------------------
//
// MOCK-MODELLED ONLY (`docs/version-selection-spec.md` section 4.2 item 6, `docs/known-issues.md`): what a real PMS lists for
// `mediaIndex=N`, and whether it flags the chosen `Media` `selected`, has never been measured. These
// tests pin that the client is safe under every answer the mock models.

/// A `/decision` body in one of four modes, for a play of version 1. Version 0 is a direct play of
/// h264/aac, version 1 a remux that transcodes the audio (hevc copied, eac3 re-encoded), so the two
/// read differently through every reader. No `Media.id`: the door is index-only.
///
/// * `chosen`: the server lists only the chosen version.
/// * `all`: it lists every version and flags the chosen one `selected`.
/// * `all-unflagged`: it lists every version and flags none.
/// * `ignore`: it ignored `mediaIndex` and answers about version 0, flagged, beside the others.
fn decision_in_mode(mode: &str, container: serde_json::Value) -> &'static [u8] {
    let v0 = |selected: bool| serde_json::json!({ "selected": selected, "Part": [{ "decision": "directplay", "Stream": [
        { "streamType": 1, "codec": "h264", "decision": "copy" }, { "streamType": 2, "codec": "aac", "decision": "copy" }] }] });
    let v1 = |selected: bool| serde_json::json!({ "selected": selected, "Part": [{ "decision": "transcode", "Stream": [
        { "streamType": 1, "codec": "hevc", "decision": "copy" }, { "streamType": 2, "codec": "eac3", "decision": "transcode" }] }] });
    let media = match mode {
        "chosen" => vec![v1(true)],
        "all" => vec![v0(false), v1(true)],
        "all-unflagged" => vec![v0(false), v1(false)],
        "ignore" => vec![v0(true), v1(false)],
        other => panic!("unknown mode {other}"),
    };
    let mut body = serde_json::json!({ "MediaContainer": { "Metadata": [{ "Media": media }] } });
    for (key, value) in container.as_object().into_iter().flatten() {
        body["MediaContainer"][key] = value.clone();
    }
    Box::leak(serde_json::to_vec(&body).unwrap().into_boxed_slice())
}

fn resolve_with_decision(body: &'static [u8]) -> Plan {
    let w = world(TWO_VERSIONS, body);
    w.resolve(&w.env(Some(PICKED)))
}

/// **U4.** A play of version 1 never takes version 0's answer for its own: where the body names the
/// chosen version (`chosen`, `all`) or can be cut to it by position (`all-unflagged`) the play is
/// the pick's remux (hevc copied, eac3 out) and not version 0's direct play; where the server
/// plainly answered about another file (`ignore`) the decision is unusable and the play takes the
/// path an unusable decision always takes, never version 0's codecs.
#[test]
#[cfg(feature = "devtriggers")]
fn a_decision_about_another_version_is_never_judged_as_the_picks() {
    for mode in ["chosen", "all", "all-unflagged"] {
        let plan = resolve_with_decision(decision_in_mode(mode, serde_json::json!({})));
        assert!(plan.url.contains("start.mkv"), "{mode}: the pick is a remux, not version 0's direct play: {}", plan.url);
        assert_eq!((plan.vcodec.as_str(), plan.acodec.as_str()), ("hevc", "eac3"), "{mode}: the pick's output codecs");
        assert_eq!(plan.media_index, 1, "{mode}");
        assert!(plan.verdict.is_none(), "{mode}");
    }
    let plan = resolve_with_decision(decision_in_mode("ignore", serde_json::json!({})));
    assert!(plan.url.contains("start.mkv"), "ignore: an unusable decision never direct-plays: {}", plan.url);
    assert_ne!(plan.vcodec, "h264", "ignore: never version 0's codecs");
    assert_ne!(plan.acodec, "aac", "ignore: never version 0's codecs");
}

/// **U4.** A mismatch must not let the container-level verdict of a body about ANOTHER file speak
/// for the chosen version: a refusing `generalDecisionCode` beside version 0's flagged answer is
/// dropped with it, so the pick is not reported as refused by the server.
#[test]
#[cfg(feature = "devtriggers")]
fn a_refusal_in_a_body_about_another_version_does_not_refuse_the_pick() {
    let refusing = serde_json::json!({
        "generalDecisionCode": 2000,
        "generalDecisionText": "Neither direct play nor conversion is available.",
        "transcodeDecisionCode": 4007,
        "transcodeDecisionText": "Cannot convert this item.",
    });
    let plan = resolve_with_decision(decision_in_mode("ignore", refusing.clone()));
    assert!(plan.verdict.is_none(), "the pick is not refused on another file's verdict");
    assert!(!plan.url.is_empty());
    // the same refusal in a body that IS about the pick still refuses it
    let plan = resolve_with_decision(decision_in_mode("all", refusing));
    assert!(plan.verdict.is_some() && plan.url.is_empty(), "a refusal of the chosen version stands");
}

/// **U4.** The default play is untouched: index 0 reads a body exactly as upstream does (the entry
/// flagged `selected`, else the first), because the door is active only for a picked index.
#[test]
#[cfg(feature = "devtriggers")]
fn a_default_play_reads_the_decision_body_exactly_as_before() {
    for (mode, direct) in [("all-unflagged", true), ("ignore", true), ("all", false)] {
        let w = world(TWO_VERSIONS, decision_in_mode(mode, serde_json::json!({})));
        let plan = w.resolve(&w.env(None));
        assert_eq!(plan.media_index, 0, "{mode}");
        // unflagged or flagged on version 0 reads version 0 (a direct play); a flag on another
        // entry is followed, as it always was
        assert_eq!(plan.url.contains("/library/parts/3134/"), direct, "{mode}: {}", plan.url);
        assert_eq!(plan.url.contains("start.mkv"), !direct, "{mode}: {}", plan.url);
    }
}

// ---- only the card's or page's resume point is clamped, never a retry's live playhead ---------------

/// **U5, scope of the clamp.** `resume_ns_for` repairs the item's STORED `viewOffset`, which was judged
/// against version 0's length. A retry (a quality or track re-resolve, a rescue) carries the LIVE
/// playhead, the only truthful position (`confirm_resume_presented`), and a viewer 88 minutes into a
/// 90-minute pick must come back at 88, not at 0. Driven the way the existing re-entry test is: a
/// refused first landing, then `retry_current_play` with the playhead.
#[test]
#[cfg(feature = "devtriggers")]
fn a_retry_of_the_picked_version_keeps_the_live_playhead_past_its_95_percent_mark() {
    let min = |m: i64| m * 60 * 1_000_000_000;
    let mut w = world(&two_lengths(), MDE_DIRECTPLAY);
    let mut meta = plx_data::stores::metadata::MetadataStore::default();
    assert!(w.request_play(&mut meta, Some(PICKED)));
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    {
        let mut slot = PLAY_SLOT.lock().unwrap_or_else(|e| e.into_inner());
        slot.as_mut().expect("the first resolve landed").plan = Plan { direct_play_mode: DirectPlayMode::Auto, ..Default::default() };
    }
    assert_eq!(pump_play(&mut w.ps, &mut meta), None, "a refused first landing");
    assert!(retry_current_play(&mut w.ps, &mut meta, min(88), None, crate::player::report::AttemptOrigin::UserRetry));
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    assert_eq!(pump_play(&mut w.ps, &mut meta), Some(min(88)), "the live playhead is not a stored viewOffset");
    assert_eq!(unpresented_resume_ns(&w.ps), min(88));
    w.finish();
}

/// **U5, scope of the clamp.** The card's point survives the "desired contract changed in flight"
/// re-resolve as the card's point: it is still clamped when the second resolve lands.
#[test]
#[cfg(feature = "devtriggers")]
fn a_cards_resume_point_is_still_clamped_after_a_contract_changed_re_resolve() {
    let mut w = world(&two_lengths(), MDE_DIRECTPLAY);
    let mut meta = plx_data::stores::metadata::MetadataStore::default();
    assert!(w.request_play(&mut meta, Some(PICKED)));
    assert!(arm_play_resume(&mut w.ps, 95 * 60 * 1_000_000_000));
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    let edit = begin_user_contract_boundary();
    assert_eq!(pump_play(&mut w.ps, &mut meta), None, "the stale plan is not installed");
    assert!(wait_resolve_workers_for_test(std::time::Duration::from_secs(10)));
    drop(edit);
    assert_eq!(pump_play(&mut w.ps, &mut meta), Some(0), "past the picked version's end: from the start");
    w.finish();
}

/// The pure decision: only a point that came from the card or page is judged against the length of
/// the version that plays.
#[test]
fn only_a_card_point_is_clamped_to_the_played_length() {
    let min = |m: i64| m * 60 * 1_000_000_000;
    assert_eq!(landing_start_ns(min(95), true, 90 * 60_000), 0);
    assert_eq!(landing_start_ns(min(88), true, 90 * 60_000), 0);
    assert_eq!(landing_start_ns(min(88), false, 90 * 60_000), min(88), "a retry's playhead stands");
    assert_eq!(landing_start_ns(min(88), true, 0), min(88), "a default play is judged by the page");
}
