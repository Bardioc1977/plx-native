//! **The Play Version list**: one row per version (`Media[]` entry) of a movie or episode, drawn
//! on two surfaces — the item menu's Play Version page (`screens::item_menu`) and the panel the
//! Detail hero's Play Version disc opens (`screens::versions_panel`). One builder, so the two can
//! never word one version two ways.
//!
//! Pure over its inputs ([`MediaVersion`]s and the caller's row identity and action), so the
//! labels and the fit are host-testable without a menu or a page.
use plx_data::pms::MediaVersion;
use plx_ui::form::{FormId, FormSection, RowKind};
use plx_ui::table::Row;

/// The list's heading: "Play Version".
pub fn title() -> &'static str {
    plx_platform::i18n::msg::browse_menu_play_version_title()
}

/// **The list as one section**, in the server's order: row `n` is `versions[n]`, identified by
/// `id(n)` and reporting `act(&versions[n])`. Every row is a button that plays.
pub fn section<Id: FormId, A, D>(
    versions: &[MediaVersion],
    id: impl Fn(u16) -> Id,
    act: impl Fn(&MediaVersion) -> A,
) -> FormSection<Id, A, D> {
    let mut sec = FormSection::new(title());
    for (n, (v, (label, detail))) in versions.iter().zip(labels(versions)).enumerate() {
        let mut row = Row::new(label);
        if !detail.is_empty() {
            row = row.detail(detail);
        }
        sec = sec.item(id(n as u16), RowKind::Button, act(v), row);
    }
    sec
}

/// What each version's row says: `"25.2 Mbps · 4K"`, over the facts that DIFFER between the
/// versions (video codec and HDR, audio, container, size), so two rows never read alike for a
/// reason the list does not show and a fact every version shares is not repeated. When a sub-line
/// would outgrow the menu ([`DETAIL_UNITS`]) the lowest-priority fact goes first (size, container,
/// audio, video), from every row alike; the rate/resolution label never does. Pure.
pub fn labels(versions: &[MediaVersion]) -> Vec<(String, String)> {
    let video = |v: &MediaVersion| {
        let codec = crate::info_panel::video_codec_name(&v.vcodec);
        if v.hdr { format!("{codec} HDR") } else { codec }
    };
    let audio = |v: &MediaVersion| {
        let codec = plx_data::metadata::friendly_codec(&v.acodec);
        let channels = match v.channels {
            1 => plx_platform::i18n::msg::widgets_tracks_mono().to_string(),
            2 => plx_platform::i18n::msg::widgets_tracks_stereo().to_string(),
            6 => "5.1".to_string(),
            8 => "7.1".to_string(),
            _ => String::new(),
        };
        join_present(&[&codec, &channels], " ")
    };
    let container = |v: &MediaVersion| v.container.to_ascii_uppercase();
    let size = |v: &MediaVersion| if v.size > 0 { plx_ui::fmt::bytes(v.size, (1, 0, 0)) } else { String::new() };
    let facts: [&dyn Fn(&MediaVersion) -> String; 4] = [&video, &audio, &container, &size];
    let differs: Vec<bool> = facts
        .iter()
        .map(|f| versions.iter().map(|v| f(v)).collect::<std::collections::HashSet<_>>().len() > 1)
        .collect();
    // The facts that tell the versions apart, in priority order (the order they are shown in): when
    // the sub-line of ANY row would outgrow the menu, the lowest-priority fact goes first, for every
    // row alike so the rows keep saying the same kinds of thing: size, then container, then audio.
    // The rate/resolution label is never touched.
    let order: Vec<usize> = (0..facts.len()).filter(|&n| differs[n]).collect();
    let detail = |v: &MediaVersion, keep: usize| {
        let shown: Vec<String> = order[..keep].iter().map(|&n| facts[n](v)).collect();
        join_present(&shown.iter().map(String::as_str).collect::<Vec<_>>(), " \u{b7} ")
    };
    let mut keep = order.len();
    while keep > 1 && versions.iter().any(|v| weight(&detail(v, keep)) > DETAIL_UNITS) {
        keep -= 1;
    }
    versions
        .iter()
        .map(|v| {
            let rate = if v.kbps > 0 { plx_ui::fmt::bitrate(v.kbps) } else { String::new() };
            let res = plx_ui::fmt::resolution(&v.resolution, v.width, v.height).unwrap_or_default();
            (join_present(&[&rate, &res], " \u{b7} "), detail(v, keep))
        })
        .collect()
}

/// How wide a row's sub-line may be, in [`weight`] units, before [`labels`] drops its
/// lowest-priority fact. The 650 px menu column holds about 527 px of sub-line (a unit is about
/// 4.1 px at the sub-line's size, so 120 units are 492 px, which leaves the table's headroom and the
/// few pixels the estimate under-counts on wide capitals such as `M`, `W` and `V`), and
/// `the_list_fits_in_every_language` measures the [`worst_case`] and [`edge_case`] lists against the
/// real column in every shipped language, which is what pins this number.
const DETAIL_UNITS: usize = 120;

/// A character's rough width in the sub-line face, in units of about 4.1 px: capitals and digits are
/// wide (about 17 px), lower case about 13, the thin letters (`i l j t f r I`) and spaces and
/// punctuation about 7. Codec and container ids
/// are capitals, which is why a plain character count over-fills a menu with them and under-fills it
/// with prose. An estimate that errs by a few percent: the fit test is what bounds it.
fn weight(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | 't' | 'f' | 'r' | 'I' => 2,
            c if c.is_uppercase() || c.is_ascii_digit() => 4,
            c if c.is_alphabetic() => 3,
            _ => 2,
        })
        .sum()
}

fn join_present(parts: &[&str], sep: &str) -> String {
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(sep)
}

/// A pair of versions that differ in video and size and share audio and container: the fixture
/// both surfaces' tests read, so a row the menu fits and the panel fits are the same row.
#[cfg(any(test, feature = "test-support"))]
pub fn sample() -> Vec<MediaVersion> {
    let common = MediaVersion { acodec: "eac3".into(), channels: 6, container: "mkv".into(), ..Default::default() };
    vec![
        MediaVersion {
            id: plx_plex::plex::MediaId(3082), kbps: 25_200, resolution: "4k".into(), vcodec: "hevc".into(),
            size: 30 << 30, hdr: true, ..common.clone()
        },
        MediaVersion {
            id: plx_plex::plex::MediaId(3086), kbps: 10_900, resolution: "1080".into(), vcodec: "h264".into(),
            size: 8 << 30, ..common
        },
    ]
}

/// The list at its WORST: every fact that can differ does (video codec and HDR, audio and layout,
/// container, size) and each is at its longest, plus a stereo row, whose layout word is translated.
/// The fixture the fit tests of both surfaces read (`sample()` shares audio and container, so its
/// sub-line never carries them).
#[cfg(any(test, feature = "test-support"))]
pub fn worst_case() -> Vec<MediaVersion> {
    let id = plx_plex::plex::MediaId;
    vec![
        MediaVersion {
            id: id(3001), kbps: 99_900, resolution: "4k".into(), vcodec: "mpeg2video".into(), hdr: true,
            acodec: "eac3".into(), channels: 6, container: "mpegts".into(), size: 987 << 30, ..Default::default()
        },
        MediaVersion {
            id: id(3002), index: 1, kbps: 10_900, resolution: "1080".into(), vcodec: "vc1".into(), hdr: false,
            acodec: "dca-ma".into(), channels: 8, container: "avi".into(), size: 123 << 30, ..Default::default()
        },
        MediaVersion {
            id: id(3004), index: 3, kbps: 8_700, resolution: "1080".into(), vcodec: "vc1".into(), hdr: true,
            acodec: "dca-ma".into(), channels: 8, container: "mpegts".into(), size: 12 << 30, ..Default::default()
        },
        MediaVersion {
            id: id(3003), index: 2, kbps: 4_300, resolution: "720".into(), vcodec: "hevc".into(), hdr: true,
            acodec: "eac3".into(), channels: 2, container: "mkv".into(), size: 100 << 30, ..Default::default()
        },
    ]
}

/// The widest list that still says all four facts: two all-capitals rows (`VC1`, `DTS-HD MA`, `AVI`,
/// `MKV`), the widest glyphs a sub-line carries, right at [`DETAIL_UNITS`].
#[cfg(any(test, feature = "test-support"))]
pub fn edge_case() -> Vec<MediaVersion> {
    let w = worst_case();
    vec![
        MediaVersion { acodec: "dts".into(), channels: 6, ..w[1].clone() },
        MediaVersion { container: "mkv".into(), ..w[2].clone() },
    ]
}

/// Exotic ids at their widest, found by the reviewer's fuzz of codec, container and size ids: the
/// capitals `M`, `W`, `V` and a `Mono` layout make a sub-line the estimate must not under-count.
#[cfg(any(test, feature = "test-support"))]
pub fn exotic_case() -> Vec<MediaVersion> {
    let id = plx_plex::plex::MediaId;
    vec![
        MediaVersion {
            id: id(3101), kbps: 9_000, resolution: "1080".into(), vcodec: "msmpeg4v3".into(), hdr: true,
            acodec: "dca-ma".into(), channels: 1, container: "webm".into(), size: 5 << 30, ..Default::default()
        },
        MediaVersion {
            id: id(3102), index: 1, kbps: 4_000, resolution: "720".into(), vcodec: "wmv3".into(), hdr: false,
            acodec: "truehd".into(), channels: 1, container: "m4v".into(), size: 888 << 30, ..Default::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_ui::form::{Form, FormTable, RowKey};
    use std::convert::Infallible;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Pos(u16);
    impl FormId for Pos {
        fn key(&self) -> RowKey {
            RowKey(u32::from(self.0))
        }
    }

    /// The label is rate and resolution; the sub-line names only what tells the versions apart
    /// (here the video codec and HDR, and the size), never a fact they share (audio, container).
    #[test]
    fn a_version_row_says_what_differs() {
        let _g = plx_platform::i18n::language_on_this_thread_for_test(plx_platform::i18n::Preference::En);
        let rows = labels(&sample());
        assert_eq!(rows[0].0, "25.2 Mbps \u{b7} 4K");
        assert_eq!(rows[1].0, "10.9 Mbps \u{b7} 1080p");
        assert_eq!(rows[0].1, format!("HEVC HDR \u{b7} {}", plx_ui::fmt::bytes(30 << 30, (1, 0, 0))));
        assert_eq!(rows[1].1, format!("H.264 \u{b7} {}", plx_ui::fmt::bytes(8 << 30, (1, 0, 0))));
        let same = vec![sample()[0].clone(), sample()[0].clone()];
        assert!(labels(&same).iter().all(|(_, detail)| detail.is_empty()), "nothing differs, nothing to say");
    }

    /// The list fits a menu panel in every shipped language, measured with the device's advances:
    /// the usual pair, and the worst case ([`worst_case`]) whose sub-line carries all four facts.
    #[test]
    fn the_list_fits_in_every_language() {
        use plx_platform::i18n::{language_on_this_thread_for_test, SHIPPED};
        let measure = plx_base::fontcov::advances::ShippedMeasure;
        let mut out = Vec::new();
        for language in SHIPPED {
            let _guard = language_on_this_thread_for_test(language);
            for (name, versions) in [("sample", sample()), ("worst", worst_case()), ("edge", edge_case()), ("exotic", exotic_case())] {
                let mut form: FormTable<Pos, plx_plex::plex::MediaId, Infallible> = FormTable::new(plx_ui::table_screen::BAND_BASE);
                form.set(Form::new().section(section(&versions, Pos, |v| v.id)), None);
                form.table.compact = false;
                let tag = format!("{} versions {name}", language.tag());
                out.extend(form.table.menu_cap_failure(&measure, &tag));
                out.extend(form.table.app_fit_failures(plx_ui::table::MENU_MAX_W, &tag));
                out.extend(form.table.app_fit_failures_hugged(&tag));
            }
        }
        plx_ui::table::assert_no_fit_failures(&out);
    }

    /// **The priority when a sub-line outgrows the menu.** The worst list cannot say all four facts
    /// in the column, so the lowest-priority ones go first, size, then container, from EVERY row
    /// alike, and the rate/resolution label is never touched; a list whose rows fit keeps all four.
    /// (`dca-ma` reads as DTS-HD MA on the way.)
    #[test]
    fn the_lowest_priority_facts_go_first_and_the_label_never() {
        let _g = plx_platform::i18n::language_on_this_thread_for_test(plx_platform::i18n::Preference::En);
        let worst = worst_case();
        let rows = labels(&worst);
        assert_eq!(rows[0].0, "99.9 Mbps \u{b7} 4K", "the label is untouched");
        assert_eq!(rows[0].1, "MPEG2VIDEO HDR \u{b7} Dolby Digital Plus 5.1", "video and audio stay, size and container go");
        assert!(rows.iter().all(|(_, d)| weight(d) <= DETAIL_UNITS && d.matches(" \u{b7} ").count() == 1), "{rows:?}");
        assert!(rows[1].1.contains("DTS-HD MA 7.1"), "dca-ma reads as DTS-HD MA: {:?}", rows[1].1);

        // 124 units is over the 120 budget: size goes, the other three stay
        let edge = labels(&edge_case());
        assert_eq!(edge[1].1, "VC1 HDR \u{b7} DTS-HD MA 7.1 \u{b7} MKV");
        // without the HDR word both rows say `VC1`, which then tells nothing apart; the other three fit
        let mut plain = edge_case();
        plain[1].hdr = false;
        assert_eq!(labels(&plain)[1].1, format!("DTS-HD MA 7.1 \u{b7} MKV \u{b7} {}", plx_ui::fmt::bytes(12 << 30, (1, 0, 0))),
            "rows that fit keep every fact that tells them apart");
    }
}
