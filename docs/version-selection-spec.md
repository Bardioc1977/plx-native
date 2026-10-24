# Version selection (multi-version media)

Spec for choosing WHICH `Media[]` version of an item plays. Status: **proposed, nothing built.**
First of the Plex-server parity specs, in priority order: version selection, playlists, library
filters, Companion (remote control), user ratings.

**Scope: movies and TV episodes on a user's own PMS.** Not the Watchlist/Discover catalog, and not
"Also available" (`screens/src/alt_sources.rs`), which chooses between *servers*, not between files
on one server. Stacked multi-part files are a different axis (§9).

## 1. The gap

A PMS item can carry several `Media[]` entries: the same title as a 4K HEVC rip and a 1080p H.264
one, an MKV and an MP4, or several encodes of one source. Official Plex clients let the viewer
choose; this client never offers the choice and always plays `Media[0]`:

- `plex::Metadata::primary_media()` / `first_part()` are `media.first()` (`plex/src/plex/models.rs`),
  and their doc comments already say "needs a version picker this client does not have yet".
- `data::metadata::Detail` projects version 0 into `part`, `vcodec`, `acodec`, `bitrate`, `width`,
  `height`, `container`, `file`, `size` and the track lists (`convert_streams(first_part().stream)`).
- `PlaybackRequest { rk, part, vcodec, acodec, .. }` (`media/src/route/decision.rs`) carries one
  Part key. `route::plan::build_stream` plays that key, or asks the transcoder for it.
- `plex/src/plex/transcoder.rs` writes `mediaIndex=0` in both the decision query and the transcode
  start query (two literals).

`docs/pms-api.md` §4 already warns: "The picker must iterate `Media[]` and choose by
codec/resolution, not take `[0]` blindly."

### Measured on the owner's library (PMS 1.43.5.11029, 2026-10-24, read-only, counts only)

- 23 of 386 movies and 27 of 1185 episodes have more than one version: 22 movies have 2, one has 9.
- Version pairs seen: 4K HEVC + 4K HEVC (two bitrates), 4K HEVC + 1080p H.264, 4K HEVC MKV + 4K HEVC
  MP4, 4K HEVC + 4K H.264, 4K HEVC + 1080p HEVC, 1080p H.264 MP4 + 1080p H.264 MKV, and the nine
  versions are all 576p H.264 MKV.
- `Media[0]` is the highest-bitrate version in 20 of 23 movies; in 3 it is not. `Media[0]` is 4K
  whenever a 4K version exists (20 of 20).
- A `Media` entry carries `id, bitrate, width, height, videoResolution, videoCodec, videoProfile,
  videoFrameRate, aspectRatio, audioCodec, audioProfile, audioChannels, container, duration,
  hasVoiceActivity, Part[]`. `optimizedForStreaming` appears on some. **There is no `selected`
  flag and no human label**: a version is described only by those technical fields and its file.
- A `Part` carries `id, key, file, size, container, duration, videoProfile, audioProfile`.
- One movie has more than one `Part` in a version (a stacked file).

So the picker cannot show a server-provided name. It has to derive a label from the technical
fields, and it must cope with versions that are identical on every field except bitrate and file.

## 2. Principles

1. **Choice is by `Media.id`, never by index alone.** The index is the wire value `mediaIndex`, but
   the stable identity (remembered, compared, logged) is `Media.id`; a re-scan can reorder the list.
2. **Version 0 stays the default.** Without a choice, behaviour is bit-for-bit today's. The default
   only changes in phase 2 (§7), behind its own measurement.
3. **Everything derived from "the version" follows the choice.** The badge, the Track information
   sheet, the About lists, the audio/subtitle lists and the route's source gate must all read the
   playing version, or the screen describes a file that is not the one playing.
4. **Lean projection.** A `Metadata` row carries the whole Media/Part/Stream/Role tree and this
   runs on a 32-bit TV (`docs/parity-gaps.md`, theme F). `Detail` carries a small `Version` per
   entry, not the tree; the streams of the chosen version are projected on demand.
5. **Shared mechanisms only** (AGENTS.md): popover + `TableView`, the `FocusEngine`, the item menu
   `Action` list, the memory door for persistence. No screen-local widget, no hand-routed keys.

## 3. Data model

New in `plx_plex::plex::models` (parsing) and `plx_data::metadata` (projection):

```rust
/// One entry of `Media[]`, projected. The identity is `media_id`; `index` is its position at fetch
/// time and the only value `mediaIndex=` ever carries.
pub struct Version {
    pub index: u32,
    pub media_id: i64,
    pub part_key: String,      // Part[0].key
    pub part_id: i64,          // Part[0].id (the PUT /library/parts/{id} target)
    pub resolution: String,    // "4k" | "1080" | "720" | "576" | "sd"
    pub width: i64, pub height: i64,
    pub vcodec: String, pub acodec: String,
    pub audio_channels: i64,
    pub container: String,
    pub bitrate_kbps: i64,
    pub size: i64,             // Part[0].size, bytes
    pub hdr: HdrKind,          // None | Hdr10 | Hlg | DolbyVision, from the video stream (is_hdr / DOVIPresent)
    pub file_name: String,     // basename of Part[0].file, for disambiguation only
}
```

- `Detail.versions: Vec<Version>` (empty or one entry ⇒ no picker), bounded by a new
  `MAX_VERSIONS = 16`. The owner's library has a 9-version item; the bound exists so a hostile or
  broken server cannot grow the record. Classify the constant in `ci/list-caps.ini`.
- `Detail.version: u32` is the **selected** position. The existing version-0 fields (`part`,
  `vcodec`, `acodec`, `bitrate`, `width`, `height`, `video_resolution`, `container`, `file`, `size`,
  the track lists) are re-derived from `versions[version]`, so the readers that exist today keep
  working and move together. A grid/hub caption (`ListingFacts`) stays version 0: a card has no
  selection.
- Leniency: `Media.id`, `bitrate` and the sizes are string-encoded on some endpoints
  (`media_technical_fields_survive_both_encodings_and_keep_every_version` already pins this for
  `Media`); every new field uses the `de_i64` adapters.
- A version with no `Part`, or an empty `Part[0].key`, is dropped from `versions` (nothing playable).

### Label

The label is composed from fields, most significant first, and never from the server path:

`4K · HEVC · HDR10 · EAC3 5.1 · MKV` (resolution, video codec, HDR kind when not SDR, audio codec
and channels, container).

Two versions whose labels collide get a second line: bitrate and size (`26.1 Mbps · 31.4 GB`). If
those collide too, the file basename elided by CHARACTER (as the Track information sheet does for
`file`), and as the last resort an ordinal (`Version 3`). Strings are i18n catalog entries in every
shipped locale (`docs/localization.md`); codec and container names are not translated.

## 4. Behaviour

### 4.1 Where the choice is offered

| surface | what | notes |
|---|---|---|
| Detail hero | a **Version** control beside Play, drawn only when `versions.len() > 1`, showing the label of the selected version | opens a popover `TableView`, exactly like `appkit/src/track_menu.rs` |
| Hold menu (`screens/src/item_menu.rs`) | a new `Action::PickVersion(rk)` row on a leaf with several versions | opens the same popover over the live screen |
| Player `…` menu | phase 2: switch version mid-playback | re-resolve at the current position, like the Quality picker |

OK in the popover commits the highlighted version and closes it; BACK closes without a change. The
popover shows one row per version: the label, the second line, and a mark on the current row. It
does not play. The viewer presses Play afterwards, so choosing and playing stay two decisions.

A **show** page has no version (episodes carry them). The show page's Play resolves the on-deck
episode, and the episode's remembered choice applies (§4.3).

### 4.2 What "selected" does to playback

`PlaybackRequest` gains `version: VersionPick { index: u32, media_id: i64 }`. Flow:

1. The request is built from `Detail.versions[Detail.version]`: `part` = that Part's key,
   `vcodec`/`acodec` = that version's codecs. `request_play` and the item-menu `Play` take the same
   path, so the choice cannot be skipped by starting from a card.
2. `route::plan::build_stream` plays that Part: direct play uses the key; the source gate
   (`devcaps`, `route::source_kbps`) judges that version's codecs, resolution and bitrate.
3. `TranscodeSpec` gains `media_index`, replacing the two `mediaIndex=0` literals in
   `plex/src/plex/transcoder.rs` (decision query and start query). `partIndex` stays 0 (§9).
4. `part_id_of(part)` derives the Part id from the key, so `PUT /library/parts/{id}` (stream
   selection) already addresses the chosen version's Part. The stream ids the track menu offers
   must come from the chosen version's Part, which is why principle 3 matters.
5. Track memory (`route/track_memory.rs`) matches by descriptor (language, kind, forced), not by
   stream id, so a remembered pick carries across versions of the same item. No change needed;
   a test pins it.
6. The `/decision` response is read by `decision_codecs` and `mde_verdict` via `media.first()`.
   **UNVERIFIED** whether PMS returns only the chosen `Media` when `mediaIndex=N` is sent. §8 probe
   P1 settles it; if it returns all of them, those readers must select by `Media.id` instead.
7. Timeline, scrobble and resume are per `ratingKey`, so they do not change: a resume point belongs
   to the item, not to a version.

### 4.3 Remembering the choice

A viewer who picks the 1080p copy of one movie expects it next time. Persisted per profile, per
item, as a **memory-door list**, following the "contract for a new setting" in
`plex/src/plex/CLAUDE.md`:

- New list row `version_memory` in `Preferences`: `{ machine_id, rating_key, media_id }`, written
  only through `ProfileMemory::upsert_with` (the `Row`-only marker, like `track_memory`).
- Account-bound (`ACCOUNT_BOUND_PREFERENCES`), soft-parsed per element (`de_soft_vec`), entry
  strings bounded, capped per profile and held to the household byte budget by eviction, never
  refusal. It joins `prefs::shed_memory`'s eviction order **after** `track_memory` and
  `subtitle_offsets` (cheapest to lose), and the 256 KiB document guard test is extended.
- Read at resolve time next to `ResolveEnv::memory`. A remembered `media_id` that is no longer in
  `Media[]` is ignored (version 0 plays) and the stale entry is dropped on the next write.
- Written by the picker only. A harness setup, an unseated boot and a dev token write nothing,
  as for `track_memory`.
- Episodes: the memory is per episode in phase 1. "Carry the same class to the next episode" is
  phase 2 (§7), because it needs a defined match rule.

### 4.4 Edge cases

- **One version:** no control, no row, no memory entry.
- **The remembered version vanished:** play version 0, no error, no toast.
- **Chosen version cannot play** (undecodable, server refuses): the existing error read-out names
  the cause, and its OK opens the quality ladder as today; the viewer can reopen the Version
  popover from the detail page. No automatic fall-back to another version in phase 1.
- **Server switch via "Also available":** the version choice is per (machine, item); a different
  server starts at its own version 0 or its own memory.
- **Trailers/extras:** unaffected; they have one version and never read the memory.
- **Offline cache / shared server:** the memory key includes `machine_id`, so a friend's server
  with the same `ratingKey` never collides (`docs/shared-servers.md` §2).

## 5. Layering

Per `ci/module-layers.ini` and `docs/module-layers.md`, references only point down:

- `Version`, its parse and `HdrKind` in `plex` (models). No UI names.
- Projection, label composition inputs and the memory row in `data`/`storage`-side prefs.
- The popover, rows and i18n in `screens` + `appkit`. Screens name no sibling screen; the app routes.
- The pick reaches the media layer as a field of `PlaybackRequest`; `media` never reads `screens`.
- No `webos`/`port` reference outside `[port webos]`.
- Logic performs no IO (`docs/sans-io.md`): the picker is a pure state machine producing a typed
  effect (`PickVersion`), and the memory write goes through the existing storage job, not a new
  worker. `ci/check-deps.sh` must not gain a ledger line.

## 6. Tests

Host (`make check`, `make test-crate C=plx_plex`, `C=plx_data`, `C=plx_media`, `make test-hostsim`):

- Parse: a two-version and a nine-version `Metadata`, both string and numeric encodings, a version
  without a `Part` (dropped), a stacked version (Part[0] used), `MAX_VERSIONS` truncation.
- Label: every pair measured in §1 produces distinct labels; identical pairs fall through bitrate,
  size, file name and ordinal in that order; CJK/long file names elide by character.
- Query: `transcode_query` and the decision query carry `mediaIndex=N` for N = 0, 1, 8 and the
  default path still emits `mediaIndex=0` byte-for-byte (the existing `engine.rs` URL fixture).
- Plan: a version-1 request plays version 1's Part key; the source gate judges version 1's codecs
  (a 4K HEVC v0 and a 1080p H.264 v1 on a device whose table cannot decode 4K).
- Detail: selecting a version re-derives badge, `part`, codecs and track lists together; none of
  them stays on version 0.
- Memory: round trip; stale `media_id` ignored; account-bound clear on sign-out; the 256 KiB guard
  evicts `version_memory` last; per-profile isolation; two servers with the same `ratingKey`.
- Screens: the control is absent for one version; focus enters and leaves through the shared
  `FocusEngine`; the popover opens from the hold menu and from the hero; BACK changes nothing.
- A text-fit test per locale for the label lines (existing `*_text_fit_tests.rs` pattern).

Simulator (`ui-sim`): the picker on `tests/mock_pms.py` with a multi-version fixture (the mock
needs a two-version and a nine-version item; add them in the same change).

Device (`tv-lock` first; `which-tier` says playback needs the set): direct play of a 4K and a 1080p
version of one fixture, the transcode path with `mediaIndex=1`, and the resume point surviving a
version change. **Boot with `--guest` against the mock, never the owner's real server**: playing
writes timeline, resume and scrobble state to a real household record (AGENTS.md).

## 7. Phases

**Phase 0 — probes (no product code).** Settle the UNVERIFIED facts in §8 against the real PMS with
read-only GETs, and record the redacted results in this file.

**Phase 1 — explicit choice.** `Version` model, `Detail.versions`, the hero control and the hold-menu
row, `PlaybackRequest.version`, `TranscodeSpec.media_index`, the memory list, mock fixtures, tests.
Default behaviour unchanged. Acceptance: on a two-version item the viewer picks version 1, it plays
(direct and transcoded), the choice survives a relaunch, and a one-version item shows no new UI.

**Phase 2 — policy and continuity.**
- Mid-playback switch from the player `…` menu at the current position.
- Default pick by playability: when `Media[0]` cannot be direct-played or decoded and another
  version can, prefer it. This is a behaviour change; it ships only with a measurement of how
  often it triggers on the owner's library and a Settings > Playback toggle, default off.
- Carry the chosen class (resolution + codec family) to the next episode of a show, including the
  Up Next successor (`resolve_playqueue`), with a defined tie-break.

## 8. UNVERIFIED facts to probe in phase 0

The owner allows testing against the real PMS as long as nothing is written to it (rules and the
reasons in `docs/playlists-spec.md` §7). Probes 1 and 2 need `/video/:/transcode/universal/*`, which
starts a server transcode session, and probe 3 needs a play queue: both create server-side state, so
they run on `tests/mock_pms.py` or with the owner's explicit OK for that session. Probes 4 to 6 are
plain `GET`s and may run on the real server. Replay fixtures stay synthetic (closed alphabet).

1. Does `/video/:/transcode/universal/decision?mediaIndex=N&...` decide on `Media[N]`, and does its
   response list only that `Media` (affects `decision_codecs`, `mde_verdict`)?
2. Does `/video/:/transcode/universal/start` with `mediaIndex=N` stream that version?
3. Does a play queue (`/playQueues`) honour a version, or carry every `Media[]` of the successor?
   What does Up Next's lean successor projection keep?
4. Is `mediaIndex` a position or a `Media.id` on 1.43.5? (Expected position; `Media.id` is the
   stable key either way.)
5. Does PMS return `Media[]` in a stable order across fetches and library scans? The memory keys on
   `media_id`, so an unstable order costs nothing, but the label ordinal would shuffle.
6. `optimizedForStreaming` values and what a version made by Plex "Optimize" (a `Media` with
   `Part.file` under the optimized-versions directory) looks like; whether it needs its own label.

## 9. Out of scope

- Stacked multi-part files (several `Part[]` in one version, one movie in the owner's library):
  playback of Part 2+ is a separate gap and needs its own spec (`partIndex`, one timeline across
  Parts). Phase 1 plays `Part[0]`, as today, and the stacked item is not special-cased.
- Server-side version management (optimize, delete, merge, split, `Media` deletion).
- Choosing audio/subtitle language per version (already handled by track memory).
- Quality ladder changes (`route::Quality`); a version choice and a bitrate ceiling stay independent.

## 10. Prose this change makes false (for `doc-claim-auditor`)

`plex/src/plex/models.rs` (`primary_media` and `first_part` doc comments), `data/src/metadata.rs`
(the "PRIMARY version's technical fields" blocks), `docs/pms-api.md` §4 (the "must iterate" note
becomes "does"), `docs/parity-gaps.md` (the version picker line under theme G), `docs/agent-reference.md`
if it lists `mediaIndex`, and `plex/src/plex/transcoder.rs`'s two `mediaIndex` call sites.
