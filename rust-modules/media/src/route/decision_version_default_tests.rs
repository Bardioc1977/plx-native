//! What the resolve asks PMS for when NO version was chosen, pinned before the version delta
//! (`docs/version-delta-plan.md`, S2) changes any of the code underneath.
//!
//! `build_stream` is run against a loopback PMS for the four shapes a default play takes (direct
//! play, container remux, re-encode, fixed HLS) and the transcript of what it asked for, plus the
//! URL it installed, is compared byte for byte. The wire carries `mediaIndex=0` in every one of
//! them; a version choice adds a different number for a chosen version and must leave these
//! transcripts exactly as they are for a play that chose none.

use super::*;
#[allow(unused_imports)]
use super::test_support::*;

/// What one resolve asked for: every request line the server saw (the token and the loopback port
/// are the fixture's own and stay), then the URL the plan installed.
#[cfg(feature = "devtriggers")]
fn transcript(requests: &[String], plan: &Plan, port: i32) -> String {
    let mask = |s: &str| mask_profile(&mask_counter(&s.replace(&format!("127.0.0.1:{port}"), "HOST")));
    let mut lines: Vec<String> = requests.iter().map(|l| mask(l.trim_end())).collect();
    lines.push(format!("plan.url {}", mask(&plan.url)));
    lines.join("\n")
}

/// The playback session id ends in a process-wide counter (`plxnative-rk-4k-<n>`), which depends on
/// how many resolves the test binary ran before this one.
#[cfg(feature = "devtriggers")]
fn mask_counter(s: &str) -> String {
    const PREFIX: &str = "plxnative-rk-4k-";
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find(PREFIX) {
        let end = at + PREFIX.len();
        out.push_str(&rest[..end]);
        out.push('N');
        rest = rest[end..].trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

/// The device-profile blob (`X-Plex-Client-Profile-Extra`) is read from the process-global device
/// table, which is not what this change touches.
#[cfg(feature = "devtriggers")]
fn mask_profile(s: &str) -> String {
    const KEY: &str = "X-Plex-Client-Profile-Extra=";
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find(KEY) {
        let from = at + KEY.len();
        out.push_str(&rest[..from]);
        out.push_str("<profile>");
        let tail = &rest[from..];
        rest = &tail[tail.find('&').unwrap_or(tail.len())..];
    }
    out.push_str(rest);
    out
}

/// Run `build_stream` for the 4K HEVC fixture against a loopback PMS answering `mde` and return
/// the transcript. `quality`/`link` select the route the way the existing resolve tests do.
#[cfg(feature = "devtriggers")]
fn resolve_transcript(
    name: &str,
    mde: &'static [u8],
    requests: usize,
    quality: Quality,
    link: Option<plx_plex::plex::probe::Location>,
) -> String {
    use std::time::Duration;
    let mut ps = crate::route::PlaybackSession::IDLE;
    let _g = fresh_registry(&mut ps);
    restore_quality(quality);
    let (port, rx, server) = plan_pms(requests, mde);
    let sid = plx_plex::plex::register_for_test(name, "127.0.0.1", port, "token", "golden-client");
    if let Some(link) = link {
        plx_plex::plex::client_for(sid).expect("registered").set_link(link);
    }
    let mut env = ResolveEnv::snapshot(
        &ps,
        plx_data::stores::metadata::MetadataStore::default().view(),
        sid,
        "rk-4k",
    );
    env.quality = quality;
    env.src_kbps = 48_000;
    env.cached_item = Some(fourk_item(sid, vec![eac3_track()]));
    let plan = build_stream(
        &plx_base::task::OffFrame::for_test(),
        "rk-4k",
        "/library/parts/36013/1/file.mkv",
        "hevc",
        "eac3",
        &env,
    );
    let seen = rx.recv_timeout(Duration::from_secs(15)).expect("PMS never saw the resolve");
    server.join().unwrap();
    let text = transcript(&seen, &plan, port);
    restore_quality(Quality::Original);
    plx_plex::plex::reset_servers_for_test();
    text
}

#[test]
#[cfg(feature = "devtriggers")]
fn a_default_direct_play_resolve_is_pinned() {
    let t = resolve_transcript("golden-direct", MDE_DIRECTPLAY, 2, Quality::Original, None);
    assert_eq!(t, GOLDEN_DIRECT);
}

#[test]
#[cfg(feature = "devtriggers")]
fn a_default_remux_resolve_is_pinned() {
    let t = resolve_transcript("golden-remux", MDE_TRANSCODE, 4, Quality::Original, None);
    assert_eq!(t, GOLDEN_REMUX);
}

#[test]
#[cfg(feature = "devtriggers")]
fn a_default_reencode_resolve_is_pinned() {
    let t = resolve_transcript("golden-reencode", MDE_TRANSCODE_VIDEO, 4, Quality::Original, None);
    assert_eq!(t, GOLDEN_REENCODE);
}

#[test]
#[cfg(feature = "devtriggers")]
fn a_default_fixed_hls_resolve_is_pinned() {
    let t = resolve_transcript(
        "golden-hls",
        EMPTY_MC,
        3,
        Quality::Auto,
        Some(plx_plex::plex::probe::Location::Relay),
    );
    assert_eq!(t, GOLDEN_HLS);
}

#[cfg(feature = "devtriggers")]
const GOLDEN_DIRECT: &str = concat!(
    "POST /playQueues?type=video&uri=server%3A%2F%2Fgolden-direct%2Fcom.plexapp.plugins.library%2Flibrary%2Fmetadata%2Frk-4k&continuous=1&shuffle=0&repeat=0&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Token=token HTTP/1.1\n",
    "GET /video/:/transcode/universal/decision?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=http&hasMDE=1&directPlay=1&directStream=1&directStreamAudio=1&mediaBufferSize=20971&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&audioStreamID=36014&subtitleStreamID=0&subtitles=none&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token HTTP/1.1\n",
    "plan.url http://HOST/library/parts/36013/1/file.mkv?X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Token=token",
);
#[cfg(feature = "devtriggers")]
const GOLDEN_REMUX: &str = concat!(
    "POST /playQueues?type=video&uri=server%3A%2F%2Fgolden-remux%2Fcom.plexapp.plugins.library%2Flibrary%2Fmetadata%2Frk-4k&continuous=1&shuffle=0&repeat=0&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Token=token HTTP/1.1\n",
    "GET /video/:/transcode/universal/decision?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=http&hasMDE=1&directPlay=1&directStream=1&directStreamAudio=1&mediaBufferSize=20971&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&audioStreamID=36014&subtitleStreamID=0&subtitles=none&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token HTTP/1.1\n",
    "PUT /library/parts/36013?allParts=1&subtitleStreamID=0&audioStreamID=36014&X-Plex-Token=token HTTP/1.1\n",
    "GET /video/:/transcode/universal/decision?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=http&directPlay=0&directStream=1&directStreamAudio=1&audioStreamID=36014&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token HTTP/1.1\n",
    "plan.url http://HOST/video/:/transcode/universal/start.mkv?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=http&directPlay=0&directStream=1&directStreamAudio=1&audioStreamID=36014&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token",
);
#[cfg(feature = "devtriggers")]
const GOLDEN_REENCODE: &str = concat!(
    "POST /playQueues?type=video&uri=server%3A%2F%2Fgolden-reencode%2Fcom.plexapp.plugins.library%2Flibrary%2Fmetadata%2Frk-4k&continuous=1&shuffle=0&repeat=0&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Token=token HTTP/1.1\n",
    "GET /video/:/transcode/universal/decision?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=http&hasMDE=1&directPlay=1&directStream=1&directStreamAudio=1&mediaBufferSize=20971&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&audioStreamID=36014&subtitleStreamID=0&subtitles=none&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token HTTP/1.1\n",
    "PUT /library/parts/36013?allParts=1&subtitleStreamID=0&audioStreamID=36014&X-Plex-Token=token HTTP/1.1\n",
    "GET /video/:/transcode/universal/decision?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=http&directPlay=0&directStream=1&videoResolution=3840x2160&maxVideoBitrate=60000&audioStreamID=36014&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token HTTP/1.1\n",
    "plan.url http://HOST/video/:/transcode/universal/start.mkv?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=http&directPlay=0&directStream=1&videoResolution=3840x2160&maxVideoBitrate=60000&audioStreamID=36014&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token",
);
#[cfg(feature = "devtriggers")]
const GOLDEN_HLS: &str = concat!(
    "POST /playQueues?type=video&uri=server%3A%2F%2Fgolden-hls%2Fcom.plexapp.plugins.library%2Flibrary%2Fmetadata%2Frk-4k&continuous=1&shuffle=0&repeat=0&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Token=token HTTP/1.1\n",
    "PUT /library/parts/36013?allParts=1&subtitleStreamID=0&audioStreamID=36014&X-Plex-Token=token HTTP/1.1\n",
    "GET /video/:/transcode/universal/decision?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=hls&directPlay=0&directStream=0&videoResolution=854x480&maxVideoBitrate=720&videoBitrate=720&peakBitrate=720&autoAdjustQuality=0&secondsPerSegment=2&videoQuality=100&mediaBufferSize=20971&fastSeek=1&directStreamAudio=0&audioStreamID=36014&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token HTTP/1.1\n",
    "plan.url http://HOST/video/:/transcode/universal/start.m3u8?path=%2Flibrary%2Fmetadata%2Frk-4k&mediaIndex=0&partIndex=0&protocol=hls&directPlay=0&directStream=0&videoResolution=854x480&maxVideoBitrate=720&videoBitrate=720&peakBitrate=720&autoAdjustQuality=0&secondsPerSegment=2&videoQuality=100&mediaBufferSize=20971&fastSeek=1&directStreamAudio=0&audioStreamID=36014&session=plxnative-rk-4k-N&X-Plex-Session-Identifier=plxnative-rk-4k-N&X-Plex-Client-Identifier=golden-client&X-Plex-Product=PlxNative&X-Plex-Version=0.0.0-test&X-Plex-Platform=webOS&X-Plex-Platform-Version=4.5&X-Plex-Device=LG%20webOS%20TV&X-Plex-Device-Name=PlxNative%20%28LG%20TV%29&X-Plex-Model=LG%20webOS%20TV&X-Plex-Provides=player&X-Plex-Client-Profile-Name=Generic&X-Plex-Client-Profile-Extra=<profile>&X-Plex-Token=token",
);
