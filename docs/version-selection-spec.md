# Version selection (multi-version media)

Spec for choosing WHICH `Media[]` version of an item plays.

Status: implemented upstream (#955 Play Version) + the delta: remembered pick and defect fixes.
Sections 3, 4.1 and the label of section 3 are superseded by #955's list; 4.2, 4.3 and 5-10 stand as
amended below.

This file was written before #955 landed. #955 built the picker surfaces and the id-based wire
(`MediaId`, `Metadata::media_version`, `PlayedVersion`, `PlaybackSession::cur_media_index`); what
this spec still governs is the delta on top of it: the per-profile remembered pick (4.3), the tick on
the version a bare Play takes, and the fixes for eight defects of the first version of the list (the
playing file's own rate and Part, the subtitle scope, the `/decision` body, the resume point, unplayable
rows, the worst row's fit, the `dca-ma` name). A section that #955 replaced keeps a one-line
"superseded by #955: <symbol>" pointer instead of a rewrite. An earlier independent implementation of
the same behaviour (with its own picker surface, not carried over) lives on the branch
`version-selection` (tags `version-selection-archive` and `version-selection-final`), with its plan,
challenger and reviewer files.

**Scope: movies and TV episodes on a user's own PMS.** Not the Watchlist/Discover catalog, and not
"Also available" (`screens/src/alt_sources.rs`), which chooses between *servers*, not between files
on one server. Stacked multi-part files are a different axis (§9).

## 1. The gap

*Historical: this section describes the client before #955 and is kept for its measurements.*

A PMS item can carry several `Media[]` entries: the same title as a 4K HEVC rip and a 1080p H.264
one, an MKV and an MP4, or several encodes of one source. Official Plex clients let the viewer
choose; this client never offers the choice and always plays `Media[0]`:

- `plex::Metadata::primary_media()` / `first_part()` are `media.first()` (`plex/src/plex/models.rs`),
  and their doc comments already say "needs a version picker this client does not have yet".
- `data::metadata::Detail` projects version 0 into `part`, `vcodec`, `acodec`, `bitrate`, `width`,
  `height`, `container`, `file`, `size`, `dur_ms` (the item's duration is `Media[0]`'s) and the track
  lists (`convert_streams(first_part().stream)`). So does `PlayingItem` (`fetch_playing_item`,
  `cached_playing`), the store the resolve, the track menu and the direct-play gate read.
- `PlaybackRequest { rk, part, vcodec, acodec, .. }` (`media/src/route/decision.rs`) carries one
  Part key. `route::plan::build_stream` plays that key, or asks the transcoder for it.
- `plex/src/plex/transcoder.rs` writes `mediaIndex=0` in two functions: `transcode_query` (the
  decision and start queries, reads `TranscodeSpec`) and `mde_decision_with_profile` (the MDE
  handshake, reads no spec).

`docs/pms-api.md` §4 already warns: "The picker must iterate `Media[]` and choose by
codec/resolution, not take `[0]` blindly."

### Measured on the owner's library (PMS 1.43.5.11029, 2026-10-24, read-only, counts only)

- 23 of 386 movies and 27 of 1185 episodes have more than one version: 22 movies have 2, one has 9.
- Version pairs seen: 4K HEVC + 4K HEVC (two bitrates), 4K HEVC + 1080p H.264, 4K HEVC MKV + 4K HEVC
  MP4, 4K HEVC + 4K H.264, 4K HEVC + 1080p HEVC, 1080p H.264 MP4 + 1080p H.264 MKV, and the nine
  versions share one resolution class, codec and container but differ in bitrate, size, duration,
  width (5 values), height (2 values) and file name. Across all multi-version items the classes
  include `480` and `sd`, containers include `avi`, codecs `mpeg4`, audio `dca`, `dca-ma`, `truehd`,
  `opus`, `mp3`.
- `Media[0]` is the highest-bitrate version in 44 of 50 items; `Media[]` is not in `Media.id` order
  (ascending in 31 of 50). The item's `duration` equals `Media[0].duration`; versions of 12 items
  differ in length by more than 1%.
- A `Media` entry carries `id` (a JSON number, unique across the library), `bitrate, width, height,
  videoResolution, videoCodec, videoProfile, videoFrameRate, aspectRatio, audioCodec, audioProfile,
  audioChannels, container, duration, hasVoiceActivity, Part[]`. `optimizedForStreaming` is the MP4
  fast-start flag (mp4/mov only), not the Plex Optimize marker. **There is no `selected` flag**
  (`Stream.selected` is per Part). A Plex Optimize version carries `proxyType` 42, `target` and
  `title`; a plain version has no human label, so the label is derived from the technical fields.
- A `Part` carries `id, key, file, size, container, duration, videoProfile, audioProfile`.
- Listing rows (section `/all`, `/children`, hubs, on-deck) embed every version in the same order but
  no `Stream[]`; HDR kind and track lists need the single-item fetch.
- One movie has more than one `Part` in a version (a stacked file); it is the nine-version movie,
  at `Media` index 2. Four single-version episodes are stacked too.

So the picker cannot show a server-provided name. It has to derive a label from the technical
fields; on the owner's library bitrate and size separate every pair (6 of 50 items collide on the
codec/resolution/container label alone, none once bitrate is added).

## 2. Principles

1. **Choice is by `Media.id`, never by index alone.** The index is the wire value `mediaIndex`, but
   the stable identity (remembered, compared, logged) is `Media.id`; a re-scan can reorder the list.
2. **Version 0 stays the default.** Without a choice, behaviour is bit-for-bit today's. The default
   only changes in phase 2 (§7), behind its own measurement.
3. **Everything that PLAYS follows the choice.** The route's source gate, the audio/subtitle lists
   of the player, the track store and the Part id that stream selection addresses read the playing
   version. *Amended:* the Detail page's own chips, the About lists and the Track information sheet
   keep describing version 0 (`docs/known-issues.md`, "Detail page facts describe version 0"); the
   tick in the version list says which version a bare Play takes.
4. **Lean projection.** A `Metadata` row carries the whole Media/Part/Stream/Role tree and this
   runs on a 32-bit TV (`docs/parity-gaps.md`, theme F). A listing or `Detail` carries a small
   `pms::MediaVersion` per entry, not the tree; the streams of the played version are projected on
   the resolve worker (`metadata::fetch_playing_item`).
5. **Shared mechanisms only** (AGENTS.md): the item menu, the `version_list` builder, the
   `FocusEngine`, the memory door for persistence. No screen-local widget, no hand-routed keys, and
   no second picker surface beside #955's.

## 3. Data model

**Superseded by #955** (`plx_plex::plex::models::MediaId`, `Metadata::media_version(Option<MediaId>)`,
`plx_data::pms::{MediaVersion, versions_of}`, `PlayedVersion`): there is no `Version` struct, no
`Detail.version` selection state and no `HdrKind`. What stands from this section, and the delta adds
to `MediaVersion`:

- The identity of a version is `Media.id` (`MediaId`), never its position. The position is the wire
  value `mediaIndex` only.
- **The Default is the `Media` at WIRE index 0, never "the first row".** `MediaVersion.index` is the
  entry's position in `Media[]` before any row is dropped; nothing may use a list position as identity
  or as "Default". With a Part-less `Media[0]` no row has index 0.
- A `Media` is a version row only when it has a playable Part: `Part[0].key` non-empty. Rows without
  one are dropped (the delta's U6). A list is offered by the one gate `pms::offers_versions`: two or
  more playable rows, or exactly one playable row that is not the Default (a Part-less `Media[0]` plus
  one playable other still gets a one-row list, so that version stays reachable); a lone Default, or
  nothing playable, is no list.
- `MediaVersion.plays` marks the row a bare Play takes for this profile: the remembered id if it is
  listed, else the row at wire index 0 if it is listed, else no row (`pms::mark_plays`, set by the
  application at the menu captures, never by the data layer).
- No `MAX_VERSIONS`: a cap on the versions a viewer can reach is a `reach` entry in
  `ci/list-caps.ini`, which the owner's rule refuses, and the transport's response cap already bounds
  the record.
- Leniency: `Media.id`, `bitrate` and the sizes may be string-encoded on some endpoints (`de_media_id`
  and the `de_i64` adapters); every new field uses them.

### Label

**Superseded by #955:** `plx_appkit::version_list::labels` ("25.2 Mbps · 4K" over the facts that
differ between the rows). It needs no new string. The delta adds two things: the worst row of the
list is fit-tested in every shipped language, and `dca-ma` reads "DTS-HD MA".

## 4. Behaviour

### 4.1 Where the choice is offered

**Superseded by #955.** The choice is offered on two surfaces and only these, both drawn by
`version_list::section`: the item menu's "Play Version…" page (`item_menu.rs`, `ItemRow::PlayVersion`
and `ItemRow::Version(n)`) and the Detail hero's Play Version disc with its panel
(`detail/hero.rs`, `versions_panel.rs`). A row plays that version now, resuming as Play does; there
is no separate selection step and no Versions pill. The Player `…` menu switch stays phase 2.

A **show** page has no version (episodes carry them). The show page's Play resolves the on-deck
episode, and the episode's remembered choice applies (§4.3).

### 4.2 What "selected" does to playback

`PlaybackRequest.version: Option<MediaId>` (#955). Flow:

1. `request_play_inner` resolves the version ONCE on the main thread: an explicit pick (a row of the
   list), else the remembered one (delta N3), else none (the Default). Every re-resolve and retry
   inherits it from `ps.request`; `request_env` only copies it. The resolve worker
   (`build_stream`) then takes `part`, `vcodec`, `acodec` and the track lists from the chosen
   `Media` of the item it fetches, so a Continue Watching card, the item-menu `Play`, Up Next and a
   launch link, which all start from version 0's `Part.key`, honour the choice too.
2. `route::plan::build_stream` plays that Part: direct play uses the key; the source gate
   (`devcaps`, `route::source_kbps`) judges that version's codecs, resolution and bitrate.
3. **Built by #955:** `TranscodeSpec.media_index` for `transcode_query`, and the `media_index`
   parameter of `mde_decision{,_forced,_with_profile}`. `partIndex` stays 0 (§9).
4. `part_id_of(part)` derives the Part id from the key, so `PUT /library/parts/{id}` (stream
   selection) already addresses the chosen version's Part. The stream ids the track menu offers
   come from the chosen version's Part (`playing_item_of(.., media)`). The side reader names the
   played Part through `PlaybackSession.cur_part` (delta U2), not the caller's argument.
5. Track memory (`route/track_memory.rs`) matches by descriptor (language, kind, forced), not by
   stream id, so a remembered pick carries across versions of the same item. No change needed;
   a test pins it.
   The quality ceiling is judged against the played version's own video rate (delta U1), and the
   subtitle search is scoped to the playing Part (delta U3).
6. The `/decision` response is read by four functions through `Metadata::decided_media()` (the entry
   flagged `selected`, else the first): `mde_verdict`, `decision_codecs`, `decision_audio`,
   `decision_copies_video`. **UNVERIFIED** whether PMS returns only the chosen `Media` when
   `mediaIndex=N` is sent, and by owner decision no real-server `decision`/`start` call is made. The
   delta's U4 narrows the body at the `Client` door by the wire index (`narrow_to_media_index`, only
   for `index != 0`, so the default path is unchanged), so all four readers are correct under every
   modelled answer; a body about another file is treated as unusable and its container-level refusal
   codes never speak for the chosen version. Verified against the mock's modes (`chosen`, `all`,
   `all-unflagged`, `ignore`) only.
7. Timeline, scrobble and resume are per `ratingKey`, so they do not change: a resume point belongs
   to the item, not to a version. The one exception is a resume point at or past 95% of the PLAYED
   version's length (versions of 12 of 50 items differ by more than 1%): the landing starts over
   (delta U5).

### 4.3 Remembering the choice

A viewer who picks the 1080p copy of one movie expects it next time. Persisted per profile, per
item, as a **memory-door list**, following the "contract for a new setting" in
`plex/src/plex/CLAUDE.md`:

- New list row `version_memory` in `Preferences`: `{ machine_id, rating_key, media_id }`, written
  only through `ProfileMemory::upsert_with` (the `Row`-only marker, like `track_memory`). It is
  written or forgotten **at a successful landing** (`pump_play`), never at the click: a pick of a row
  whose wire `index != 0` remembers; a pick of the Default (wire index 0), or of an id the item no
  longer lists, forgets; a refused or failed plan, a retry landing and an extra, trailer or preview
  landing write nothing; an item that could not be fetched touches nothing.
- Account-bound (`ACCOUNT_BOUND_PREFERENCES`), soft-parsed per element (`de_soft_vec`), entry
  strings bounded, capped per profile and held to the household byte budget by eviction, never
  refusal. It joins `prefs::shed_memory`'s eviction order **last**, after `track_memory`,
  `subtitle_offsets` and `library_sorts` (losing a pick silently plays another file), and the
  256 KiB document guard test is extended. Proposed bounds: 128 entries per profile, 24 KiB
  household.
- Read ONCE in `request_play_inner` (fresh plays of a film or episode only) and stored on the request.
  A remembered `media_id` that is no longer in `Media[]` is ignored (version 0 plays) and the stale
  entry is dropped at that landing. An explicit pick never consults the memory.
- A harness setup, an unseated boot, `--guest` and a dev token write nothing, as for `track_memory`.
- With a Part-less `Media[0]` the Default cannot play, so there is no way back to it and a remembered
  pick of `Media[1]` stays (`docs/known-issues.md`).
- Episodes: the memory is per episode in phase 1. "Carry the same class to the next episode" is
  phase 2 (§7), because it needs a defined match rule.

### 4.4 Edge cases

- **One version:** no control, no row, no memory entry.
- **The remembered version vanished:** play version 0, no error, no toast.
- **Chosen version cannot play** (undecodable, server refuses): the existing error read-out names
  the cause, and its OK opens the quality ladder as today; the viewer can reopen the version list
  from the item menu or the detail page. No automatic fall-back to another version in phase 1, and a
  pick that cannot play is not remembered.
- **Server switch via "Also available":** the version choice is per (machine, item); a different
  server starts at its own version 0 or its own memory.
- **Trailers/extras:** unaffected; they have one version and never read the memory.
- **Offline cache / shared server:** the memory key includes `machine_id`, so a friend's server
  with the same `ratingKey` never collides (`docs/shared-servers.md` §2).

## 5. Layering

Per `ci/module-layers.ini` and `docs/module-layers.md`, references only point down:

- `MediaId`, `Media` and the parse in `plex` (models). No UI names.
- `MediaVersion`, `mark_plays` and the memory row in `data`/`session`-side prefs.
- The list, rows and i18n in `screens` + `appkit`. Screens name no sibling screen; the app routes.
- The pick reaches the media layer as a field of `PlaybackRequest`; `media` never reads `screens`.
- No `webos`/`port` reference outside `[port webos]`.
- Logic performs no IO (`docs/sans-io.md`): the memory write goes through the media door
  (`route/decision_memory.rs`) and the existing storage job, not a new worker.
  `ci/check-deps.sh` must not gain a ledger line.

## 6. Tests

Host (`make check`, `make test-crate C=plx_plex`, `C=plx_data`, `C=plx_media`, `make test-hostsim`).
The regression-critical tests are written first and observed failing (AGENTS.md):

- Parse: a two-version and a nine-version `Metadata`, both string and numeric encodings, an
  unplayable `Media` (no `Part`, or an empty `Part[0].key`) dropped, a `Media[0]` without a `Part`
  giving rows whose `index` starts at 1, a stacked version (Part[0] used, not special-cased). There is
  no `MAX_VERSIONS`, so no truncation test.
- Query: `transcode_query`, `mde_decision` and `mde_decision_forced` carry `mediaIndex=N`, and the
  default path still emits `mediaIndex=0` byte-for-byte (goldens written first).
- Plan: a version-1 request plays version 1's Part key; the source gate judges version 1's codecs; a
  fixed quality rung judges version 1's own video rate (U1); the side reader names version 1's Part
  (U2); a retranscode whose `/decision` cannot be read uses the picked file's codecs.
- Decision door: the four mock modes for a version-1 play, and a mismatch with a refusing
  `generalDecisionCode` must not refuse the chosen version (U4).
- Resume: a resume point past 95% of the played version's length starts over (U5).
- Memory: round trip; stale `media_id` dropped; the remembered version survives a re-resolve before
  the landing and a retry after a refused plan; a pick of wire index 0 forgets; a pick of `Media[1]`
  with a Part-less `Media[0]` is remembered; a failed fetch keeps the pick; account-bound clear on
  sign-out; the 256 KiB guard evicts `version_memory` last; per-profile isolation; two servers with
  the same `ratingKey`; an unseated boot reads and writes nothing; a seated profile with no pick plays
  byte-for-byte as before.
- Screens: the tick sits on `plays` and the cursor opens on it; no row is ticked when the Default is
  unplayable; the worst row of the list fits in every shipped language (U7).
- Subtitle search: the scope follows the playing Part (U3).

Simulator (`ui-sim`): the list on `tests/mock_pms.py --multi-version` (the nine-version item, the
tick on the remembered row).

Device (`tv-lock` first; `which-tier` says playback needs the set): direct play of a 4K and a 1080p
version of one fixture, the transcode path with `mediaIndex=1`, and the resume point. **Boot with
`--guest` against the mock, never the owner's real server** (`--guest` never seats a profile, so the
memory legs use `tv-session.sh up --mock-plextv`): playing writes timeline, resume and scrobble state
to a real household record (AGENTS.md).

## 7. Phases

**Phase 0 - probes (no product code).** Done: the redacted counts F1-F14 are in `docs/pms-api.md`.

**Phase 1 - explicit choice.** Built by #955 (the surfaces, the id on the wire, the picked version's
Part, codecs and tracks). The delta adds the per-profile memory, the tick, and the fixes named in
the status line. Default behaviour unchanged. Acceptance: on a two-version item the viewer
picks version 1, it plays (direct and transcoded), the choice survives a relaunch, and a one-version
item shows no new UI.

**Phase 2 - policy and continuity.**
- Mid-playback switch from the player `...` menu at the current position.
- Default pick by playability: when `Media[0]` cannot be direct-played or decoded and another
  version can, prefer it. This is a behaviour change; it ships only with a measurement of how
  often it triggers on the owner's library and a Settings > Playback toggle, default off.
- Carry the chosen class (resolution + codec family) to the next episode of a show, including the
  Up Next successor, with a defined tie-break. (Until then the successor recalls its own episode's
  pick, per episode.)
- The Detail page's chips, About and Track information following the remembered version (a fetch-time
  projection of `Detail`; today they stay version 0's, `docs/known-issues.md`).

## 8. UNVERIFIED facts to probe in phase 0

The owner allows testing against the real PMS as long as nothing is written to it (rules and the
reasons in `docs/pms-api.md`). Probes 1 and 2 need `/video/:/transcode/universal/*`, which
starts a server transcode session, and probe 3 needs a play queue: both create server-side state, so
by owner decision they run on `tests/mock_pms.py` only (modes `chosen`, `all`, `all-unflagged`,
`ignore`) and stay UNVERIFIED on the real PMS. Probes 4 to 6 are plain `GET`s. Status: **5 and 6
resolved** (the redacted counts F1 to F14 are recorded in `docs/pms-api.md`, section 4,
"Multi-version items"; order is stable between fetches and endpoints, `Media.id` is the identity;
`optimizedForStreaming` is the MP4 fast-start flag and an Optimize version carries `proxyType` 42,
`target` and `title`); **4 partly**: the OpenAPI describes `mediaIndex` as an index and `-1` or absence
as "let the server choose". Replay fixtures stay synthetic (closed alphabet).

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

- Stacked multi-part files (several `Part[]` in one version; the owner's nine-version movie has one
  stacked version, four single-version episodes are stacked): playback of Part 2+ is a separate gap
  and needs its own spec (`partIndex`, one timeline across Parts). Phase 1 plays `Part[0]`, as today,
  and a stacked version is not special-cased, whether or not it is `Media[0]` (`docs/known-issues.md`).
- Server-side version management (optimize, delete, merge, split, `Media` deletion).
- Choosing audio/subtitle language per version (already handled by track memory).
- Quality ladder changes (`route::Quality`); a version choice and a bitrate ceiling stay independent.

## 10. Prose this change makes false (for `doc-claim-auditor`)

The sentences below said something true before the delta and are false (or were reworded) after it.
An audit looks for contradictions only, never for missing documentation. `AGENTS.md` and
`docs/agent-reference.md` mention none of this.

- `PlaybackRequest.version`, `ResolveEnv.version` and `request_play`'s docs in `media/src/route/decision.rs`
  and `plan.rs`: `None` means "the remembered version, else the default", not "the default".
  `request_env`'s comment "Both describe the default version" and every claim that the quality
  ceiling source rate is zeroed for a picked version.
- `data/src/pms.rs`: the docs of `versions_of` (unplayable entries are dropped; a list needs two playable
  rows or one that is not the Default), `MediaVersion.hdr`, `MediaVersion.index` and `MediaVersion.plays`;
  `data/src/metadata.rs`: `Detail.versions` and the "PRIMARY version" blocks.
- `screens/src/versions_panel.rs` and `screens/src/item_menu.rs`: "the first row" is not the Default and
  not where the cursor opens (it opens on the ticked row).
- `rust-modules/plex/src/plex/CLAUDE.md`: the preference inventory (four account-bound memory lists),
  the `ACCOUNT_BOUND_PREFERENCES` description and the document guard's shed order (`version_memory`
  last).
- `docs/parity-gaps.md`: the three LANDED entries for Play Version ("next-episode autoplay plays the
  default", "Still open: the device has not played a picked version yet", the "two or more versions"
  gate) and the nearby lines on the hero chips.
- `docs/architecture-review-2026-07-26.md` (the paragraph on a version picker) and `docs/pms-api.md`
  ("The picker must iterate `Media[]`", the notes on `mediaIndex` and on `Media.selected`).
- `tests/README.md`: the mock flags (`--multi-version` and `--version-decision` replace a second flag).
- This spec itself: the status line, 3, 4.1, 4.2 item 6 and 4.3.

Not audit targets: `docs/plex-api.md` and `docs/plex-api-catalog.md`, which describe `mediaIndex` only as
a query parameter.
