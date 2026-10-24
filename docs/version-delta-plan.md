# Version selection: the delta on top of Play Version (#955)

Status: plan, nothing built. Written by the architect lane in the worktree `version-delta`, cut from
`origin/main`, fast-forwarded to `fba7a811` (it contains `b9377270`, "Play Version", PR #955; `ef3e55a4`, #1013, and
`fba7a811`, #1015, came after the first draft and are accounted for below).

Owner ruling that frames this document: **upstream's UX and mechanisms are the base** (item-menu
"Play Version…" page, hero Play Version disc and its panel, `MediaId`, `Metadata::media_version`,
`PlayedVersion`, `cur_media_index`). We build **only the delta** between that and our independently
specified and implemented feature (`docs/version-selection-spec.md`, plan, challenger and reviewer
files, all on branch `version-selection`, 32 commits over `origin/main`, of which 5 are the
`parity-specs` ancestry). **No second picker surface is reintroduced.**

How evidence is cited: `file:symbol` against the tree at `fba7a811` (lines drift; grep the symbol).
"Observed" means a command was run in this worktree; everything else is code reading. The only
command run was `make test-crate C=plx_media T=version_tests` (upstream's 5 tests: `5 passed; 0
failed`), to prove the build works here. No real-server request of any kind was made for this plan
(the facts F1-F14 recorded on our branch are counts only and are reused as they are); no `decision`,
`start` or play queue is needed or allowed (spec section 8, decision D10 stands).

## 0. Result in ten lines

1. Upstream already does the core well: id-based identity, `mediaIndex` on the MDE ask / transcode
   decision / start URL, the picked version's part, codecs, track store and part id, version kept by
   retry and quality changes (it rides `ps.request`), `Plan.src_vcodec/src_acodec` rebased (our B2),
   lenient `Media.id` parse. Those parts of our branch are **DROPPED**.
2. Upstream has **eight defects/gaps** against our approved requirements (section 3): U1 the
   quality-ceiling source rate is zeroed for every picked version (a fixed rung always re-encodes it,
   the Default row included); U2 `side_reader_target` reads the default version's Part; U3 the
   subtitle search/install scope reads every version's streams; U4 the `/decision` body is read as
   "selected, else `Media[0]`", which is the default version when the server lists all versions and
   flags none; U5 the resume point is judged against version 0's length; U6 a version without a
   playable Part is offered and its pick plays an empty Part key; U7 the version list's fit is never
   tested with its worst row; U8 `dca-ma` reads "DCA-MA".
3. The genuine feature delta is the **per-profile remembered pick** (spec 4.3): `version_memory`
   preference list (our steps 4-5 port nearly verbatim), recalled **once** in `request_play_inner`
   (our B1), written/forgotten **at a successful landing** (a pick of the row at wire index 0 forgets), plus a
   tick on "the version Play takes" in the existing list.
4. Not carried over: `Detail.versions/version`, `select_version`, `carry_version`, `SelectVersion`,
   `PlayingItem.source`, the Versions pill, `version_menu`, `version_label`, the `Version {n}`/`Default`
   strings, `HdrKind`, the 7th hero slot, the `fmt_layout` hoist. Upstream's `version_list` replaces
   the composer; its rows are at least as informative (section 2.9).
5. No new surface, no new i18n key, **no screen-shape change is designed**, so no replay re-record is
   expected (section 6, S16 is conditional and then happens once, at the end).
6. Ledgers: none grows. `ci/list-caps.ini` gains the two `work` sections for the memory list (as our
   step 4). No `Makefile`/`Cargo.toml`/`.github` change, so no `build-bench` table.
7. Eleven implementation steps for the defects and the feature (S1-S11), each one commit with its gate,
   regression test first and observed red; docs, mock, verification and the optional re-record after.
8. The mock is reconciled by **extending upstream's `--multi-version`** (flag kept, our extra fixture
   items, `mediaIndex` handling and the decision modes added); `--version-fixture` is not kept.
9. Open decisions for the owner (section 8): sticky pick (O1), whether the hero chips follow the
   remembered version (O2), include the `/decision` door (O3), which docs land (O4).
10. The real server's behaviour for `mediaIndex=N` stays UNVERIFIED by decision (D10); every claim in
    S5 is "client correct under every modelled answer", never "PMS does X".

## 1. What upstream built (the map to read before touching anything)

| layer | symbols (all `file:symbol`) |
|---|---|
| plex | `models.rs:MediaId` (transparent `i64`, `de_media_id` = `de_i64`), `Media.{id, selected, audio_channels}`, `Metadata::media_version(Option<MediaId>) -> Option<(usize, &Media)>` (None = `Media[0]`; Some = lookup by id, `None` if gone), `Metadata::default_part`, `Metadata::decided_media` (the `/decision` body's `selected` entry, else the first), `params.rs:TranscodeSpec.media_index`, `transcoder.rs:transcode_query`, `mde_decision{,_forced,_with_profile}(.., media_index, ..)` |
| data | `pms.rs:MediaVersion` / `versions_of` (all `Media[]` when there are >= 2 and none has id 0), `PmsMovie.versions`, `Episode.versions`, `Detail.versions`; `metadata.rs:fetch_playing_item(client, sid, rk, want) -> Option<(PlayingItem, Option<PlayedVersion>)>` (since #1013 the caller passes the `&'static Client`; `build_stream` already holds it), `PlayedVersion {index, id, part, vcodec, acodec}`, `playing_version_from`, `playing_item_of(.., media)` |
| media | `decision.rs:PlaybackRequest.version`, `request_play(.., version)`, `request_play_movie(.., version)`, `request_env` (sets `env.version`; **zeroes `cached_item` and `src_kbps`**), `ResolveEnv.version`, `PlaybackSession.cur_media_index` (+ `HlsAbrControl`, `RebasePlan`, `OriginalRecoveryPlan`, `RetranscodeClaimInputs`, `measure_remote_remux` carry it), `server_decision/forced_server_decision(.., media_index, ..)`; `plan.rs:build_stream` (picked branch shadows `part/vcodec/acodec`, sets `plan.media_index/part_id/src_vcodec/src_acodec`), `Plan.media_index`, `transcode_spec{,_carrying_burn}(.., media_index, ..)`, `decision_audio/decision_codecs/decision_copies_video` and `mde_verdict` (now via `decided_media`) |
| appkit | `version_list.rs:{section, labels, sample, title}` (one builder for both surfaces) |
| screens | `item_menu.rs:{Action::ChooseVersion, Action::PlayVersion{rk, media}, ItemRow::{PlayVersion, Version(n)}, build_versions}`, `versions_panel.rs` (panel anchored to the disc), `detail/hero.rs:{HeroCtl::Version, ELEM_VERSION, DISCS}`, `detail/mod.rs:{hero_versions, play_hero(.., version)}`, `registry.rs:{ContentPanel::Versions, AppArg::VersionsPanel, AppMsg::PlayVersion, PlayIntent::Item{version}, PlayIntent::Movie{item, version}}` |
| app | `input.rs` (`Action::PlayVersion` dispatch, `loaded_episode`/`captured_card` with a version), `playback.rs` (`request_loaded_episode(.., version)`, `play_item_now_with(.., version, ..)`), `bridge.rs` (episode menu captures `ep.versions`) |
| mock / tests | `tests/mock_pms.py --multi-version` (second 4K version on the first movie and first episode, ids from 9_900_000), no `mediaIndex` handling in decision/start, no test of the flag; `media/src/route/decision_version_tests.rs` (5 tests), plex `models.rs`/`transcoder.rs` tests, `item_menu` and `version_list` tests |

Behaviour upstream states in `docs/parity-gaps.md`: a row plays now (no selected-version state), the
hero chips and About/Track information keep describing version 0, retry and quality keep the
version, next-episode autoplay plays the default, "the device has not played a picked version yet".

## 2. Requirement-by-requirement comparison

Verdicts: **OK** (handled, evidence given), **PARTIAL**, **DEFECT** (fix with a failing test first),
**NOT BUILT** (feature delta), **N/A** (upstream has no such concept).

### 2.1 B1 - remembered pick lost on re-resolve (`pump_play` / retry)

Upstream has no memory, so there is nothing to lose; the **explicit** pick is safe by construction:
`request_play_inner` stores `request.version` on `ps.request`, and both re-entries pass
`ps.request.clone()` (`decision.rs:pump_play`, "desired contract changed in flight", and
`retry_current_play`). The only test, `the_picked_version_rides_the_request_and_the_landing`, covers
`request_env` and `apply_plan`, **not** those two re-entries. Verdict: explicit **OK** (add the two
re-entry tests as characterisation, S3); remembered **NOT BUILT** (N3 must resolve the memory ONCE into
`request.version` inside `request_play_inner`, never in `request_env` and never only for
`env.fresh_play`; this is exactly the bug our challenger's B1 caught).

### 2.2 B2 - `Plan.src_vcodec/src_acodec` stay version 0's

**OK.** `plan.rs:build_stream` reassigns both in the picked branch (`plan.src_vcodec = vcodec...`), and
`a_picked_version_direct_plays_its_own_part_after_asking_about_it` asserts `("hevc","eac3")`. Not
covered: a retranscode whose `/decision` cannot be read uses them after `apply_plan`. Port our test
`the_files_codecs_...` as characterisation (S3, test only).

### 2.3 B3 - `src_kbps` meaning and zeroing

**DEFECT U1.** `decision.rs:request_env` sets `env.src_kbps = 0` for any picked version.
`plx_plex::plex::Ceiling::admits` (`params.rs`) "fails CLOSED" on 0, `route::quality_policy`
(`plan.rs`) then answers `direct_play: false, remux: false` for every fixed rung, and `plan.src_measure`
(`= (env.src_kbps, w, h)`, installed as `cur_src`) keeps the 0 for every later quality change of that
playback. Result: with a fixed rung (not Auto, not Original) a picked version **always re-encodes**,
also when it is the row at wire index 0 (the Default) and also when it is far below the ceiling. Our B3 rule is the
fix: the rate is "the VIDEO stream's own, whole file only as fallback" (`plan.rs:source_kbps`), measured
for the version that plays.

### 2.4 B4 - Default row is index 0; Part-less or stacked `Media[0]`

N/A for "Default" (upstream draws no Default marker and keeps no selection: rows are `Media[]` in the
server's order and a row plays now). **Stacked** versions are not special-cased (`Part[0]` plays), which
equals our D7 ruling. **Part-less**: `pms.rs:versions_of` keeps an entry with no `Part`, and
`metadata.rs:playing_version_from` then returns `part: ""`, which `build_stream` shadows over the real
argument. **DEFECT U6** (small).

### 2.5 `/decision` narrowing (our `narrowed_to`, id with positional fallback, no refusal-code leak)

**PARTIAL, DEFECT U4.** Upstream reads the body through `Metadata::decided_media()` (the entry flagged
`selected`, else `Media[0]`). That is right when PMS lists only the chosen entry, or lists all and flags
the chosen one. It is **wrong when PMS lists all versions and flags none**: the four readers
(`mde_verdict`, `decision_codecs`, `decision_audio`, `decision_copies_video`) silently judge version 0
for a version-N play. The only test (`a_decision_reads_the_selected_version`) is a pure DTO test; no
resolve-level test feeds a multi-entry body. The refusal-code leak (container-level
`generalDecisionCode` of a body about another file reaching `refusal()`) is unreachable upstream only
because nothing ever detects a mismatch; once U4 is fixed it must stay closed (our batch-A/B finding).
Evidence level: all of this is **mock-modelled**, the real answer is UNVERIFIED (D10).

### 2.6 Subtitle search scope follows the playing Part (`SubSearchView::wants_open`)

**DEFECT U3.** `subsearch.rs:subtitle_rows` reads `item.media.iter().flat_map(|m| &m.part)`, every
version's streams; `identify` can adopt another version's new stream as the downloaded one. The open
search also outlives a playback (`SubSearchCmd::Open` is sent only when `!ours`), so playing the same
item as another version leaves the old scope standing (our batch-B blocking finding). This is also a
latent bug for the default version of any multi-version item before #955.

### 2.7 Track list / part id follow the chosen version

**OK.** `PlayingItem` is built from the picked `Media` (`playing_item_of(.., media)`); `plan.part_id =
part_id_of(part)`; the test asserts the audio stream ids (31302), the track store width and that every
`PUT /library/parts/` names the picked Part, on both the direct-play and the transcode path.

### 2.8 Up Next / successor, side reader, `Media.id` parsing

- **Up Next / successor:** by design the default (`request_play_up_next` -> `request_play(.., None)`;
  `QueueRow::of` keeps `Media[0]`). **OK** for upstream; with N3 the successor resolves through the same
  recall, so a remembered episode version applies there too (per episode; "carry the class" stays Phase 2).
- **Side reader target:** **DEFECT U2.** `decision.rs:side_reader_target` returns `part:
  request.part.clone()`; `request.part` is the caller's argument (the default's Part key from the card,
  the episode row or `Detail.part`), and `apply_plan` rebases only `cur_media_index`. A remux with a
  client-drawn embedded subtitle of a picked version would read version 0's file.
- **`Media.id` parsing:** **OK.** `de_media_id` over `de_i64`; `a_version_is_resolved_by_id_and_a_lost_one_is_not_replaced`
  pins string `"3086"`, number `3082`, and an id the answer lost. An absent id is 0 and
  `versions_of` then offers no list at all (one 0 id suppresses the picker; acceptable, F2 saw ids on
  5148 of 5148 entries).

### 2.9 Hero label, row labels, resume

- **Hero label reflecting the selected version:** N/A upstream (no selected-version state; the chips
  describe version 0 and say nothing else). It becomes a question only once a pick is remembered: O2.
- **Row labels (upstream `version_list::labels` vs our composer):** `"25.2 Mbps · 4K"` over "the facts that
  differ" (video codec + HDR, audio, container, size). It is as informative as ours on the measured data
  (F8: bitrate and size separate every pair; 0 of 50 items collide) and it needs no new string. Gaps,
  none worth a second composer: HDR is a bool (no Dolby Vision vs HDR10), a listing row has no `Stream[]`
  so a card menu never shows HDR (`MediaVersion.hdr` doc says so), no runtime, no ordinal/file tiebreak.
  **Decision: DROP our composer**; U7 and U8 are the two things to add.
- **Resume (our D8):** **DEFECT U5.** `detail/mod.rs:play_hero` uses `play_resume_ns(from_start,
  d.resume_ms, d.dur_ms)` with version 0's length, `input.rs` uses `ep.dur_ms`, and `pump_play` returns
  the resume unchanged. F10: 12 of 50 items have versions differing by more than 1%, 7 by 10% or more.

## 3. Upstream defects, each with its failing test first

Convention for every defect: write the test, **run it and record the failing assertion text**, then
the fix, then the passing `test result:` line (AGENTS: regression test first). Each is independent of
the others unless stated.

### U1 - quality ceiling is judged against 0 for every picked version

- Test (new, `media/src/route/decision_version_tests.rs`):
  `a_picked_version_is_measured_from_its_own_video_stream_under_a_quality_rung`. The existing
  two-version fixture gets a video-stream `bitrate` on version 1 (say 6000 kbps, 1080p) and
  `env.quality` set to a fixed rung above it. Assert `plan.src_measure.0 == 6000` and that the plan is
  not the forced re-encode (`quality_policy` unrestricted: direct play or remux per the MDE answer). A
  second case: a rung below 6000 still denies direct play. A third: picking the row at wire index 0 behaves like
  a default play of the same item (same route and spec). **Red against HEAD:** `src_measure.0 == 0` and
  a re-encode.
- The existing assertion `env.cached_item.is_none() && env.src_kbps == 0` in
  `the_picked_version_rides_the_request_and_the_landing` pins the defect; it changes to
  `cached_item.is_none()` only.
- Fix: `PlayedVersion` gains `video_kbps` (the picked `Media`'s video stream bitrate, whole-file
  `Media.bitrate` as the fallback; `convert_streams(..).video` already carries it). `build_stream`
  computes `let src_kbps = played.as_ref().map_or(env.src_kbps, |v| v.video_kbps);` right after
  `played` is known and reads it at the five `env.src_kbps` sites (`plan.rs`: the
  `source_transport_kbps` fallback, `tentative_quality`, the second `quality_policy`, `plan.src_measure`,
  the log line). `request_env` keeps `cached_item = None` for a picked version and **stops zeroing
  `src_kbps`**. One rule, two callers: `route::plan::source_kbps(&Detail)` and the new computation call
  one function in `plx_data::metadata` (extend in place; do not copy the video-else-whole rule).
- Layering: data + media only. Gate: `make test-crate C=plx_data`, `C=plx_media`,
  `make test-hostsim C=plx_media`, `make check-nodef`.

### U2 - the side reader reads the default version's Part

- Test (port of our `6011723b`, `media/src/route/decision_audio_enhancement_tests.rs`):
  `side_reader_target_names_the_played_part_of_a_picked_version`: land a plan for version 1, assert
  `side_reader_target(ps).part` is version 1's key. **Red:** it is `request.part` (version 0's).
- Fix: `Plan.part: String` (the Part key that plays; for a default play the argument, so nothing
  changes there) and `PlaybackSession.cur_part: String` installed by `apply_plan` beside `cur_part_id`
  (three construction sites, as `cur_media_index` had: `IDLE`, the destructure, the clone);
  `side_reader_target` reads `cur_part`. **Why a session field and not an `apply_plan` rebase of
  `ps.request`** (what our branch did): a retry re-sends `ps.request`; if the picked version vanished
  between two attempts the rebased request would then play the vanished version's old Part key as "the
  default". The request stays what the caller asked for.
- Gate: `make test-crate C=plx_media`, `make test-hostsim C=plx_media`, `make check-nodef`.

### U3 - subtitle search scope reads every version

- Tests (port of `dcd61eac` + `392897c5`, `data/src/subsearch.rs`): `stream_rows_are_scoped_to_the_playing_part`
  (two `Media`, different Part ids, a new stream appears in the other version only: `identify`
  adopts nothing), `a_same_item_open_with_another_part_moves_the_scope_and_keeps_the_answer`,
  `the_player_reopens_for_another_item_or_another_part_of_this_one` (`SubSearchView::wants_open`).
  **Red:** the first adopts the other version's stream.
- Fix, smaller than ours: **no `PlayingItem` change.** The scope value is the playing Part id, which
  the player overlay already reaches: `plx_media::route::cur_part_id(ps)` (installed from
  `plan.part_id`, so it is the picked version's). `SubSearchCmd::Open` gains `part_id`, `Job::Download`
  and `Job::Refresh` carry it, `subtitle_rows(client, rk, part_id)` scopes only when the fetched item
  lists **more than one `Media`** (`item.media.len() > 1`; a one-version item, stacked or not, keeps
  today's all-Parts read). `overlay.rs` sends `Open` when `view.wants_open(sid, &rk, part_id)` and a
  re-open of the item already searched carries `view.lang()`. `subsearch.rs` is a `MACHINES` file: the
  change adds a parameter, no registry read, no ledger key.
- Gate: `make test-crate C=plx_data T=subsearch`, `make lint` (the overlay), `make test-crate C=plx_screens`.

### U4 - `/decision` read as "selected, else `Media[0]`"

- Tests: plex (`transcoder.rs` / `models.rs`) pure tests of the door below; media (`decision_version_tests.rs`)
  resolve-level tests against a loopback PMS in four modes (`chosen`, `all` flagged, `all-unflagged`,
  `ignore` = the server answers version 0 flagged whatever was asked) for a version-1 play. Assert, in
  each mode, `decision_codecs`, `decision_audio`, `decision_copies_video` and `mde_verdict` describe
  version 1 or the play takes the "unusable decision" path; never version 0's codecs. A last test: a
  mismatch with a refusing `generalDecisionCode` must not turn into a refusal of the chosen version.
  **Red:** `all-unflagged` returns version 0's codecs.
- Fix (index-only, so no id is threaded through the ~30 `media_index` sites upstream touched):
  `MediaContainer::narrow_to_media_index(&mut self, index: usize)` in `models.rs`, called from the three
  `Client` functions that return a decision body (`mde_decision_with_profile`, `transcode_decision`,
  `transcode_decision_until`, which all already hold the index) **only when `index != 0`**, so the default
  path stays byte-for-byte upstream's. Rule: a body listing at most one `Media` is left alone; with
  several, exactly one flagged `selected` is kept if it is at `index` and is a mismatch otherwise; none
  or several flagged: keep `Media[index]` if in range, else mismatch. A mismatch empties `Metadata`
  **and drops the container-level verdict codes and sentences** (so `refusal()` cannot speak for another
  file; our batch-B correction), and logs `decision: wanted media index <i>, server listed <n>` through
  `plx_base::eventlog` (counts only). `decided_media` stays as the reader of the now single entry.
- This is mock-verified only (D10). Gate: `make test-crate C=plx_plex`, `C=plx_media`, `make check-nodef`.
  Whether to land it at all is O3.

### U5 - resume point judged against version 0's length

- Tests: data (`metadata.rs`, beside `resume_ns`): `resume_ns_for(resume_ns, played_dur_ms)` is 0 at
  or past 95% of the played version, unchanged below, unchanged for `played_dur_ms <= 0`; media
  (`decision_version_tests.rs`): `a_resume_point_past_the_end_of_the_picked_version_starts_over`
  (item-level resume 100 min, version 1 is 90 min: `pump_play` returns `Some(0)`; a default play is
  untouched). **Red:** `pump_play` returns the unclamped value.
- Fix (port of our D8, card path and hero path in one place): `Media.duration` (`de_i64`) joins the plex
  DTO, `PlayedVersion.dur_ms`, `Plan.played_dur_ms` (0 = unknown / default), and `pump_play` returns
  `ok.then_some(resume_ns_for(resume_ns, plan_played_dur))` read **before** `apply_plan` consumes the
  plan; it also zeroes `ps.requested_resume_ns` in that case so a retry cannot reopen on the credits
  (our batch-B note). `Detail.dur_ms` and the UI's own `play_resume_ns` stay version 0's (no selection
  state exists to re-derive them); the landing clamp is the one place that knows the played length.
  The Info card's `NowPlaying.dur_ms` stays version 0's on a picked play: cosmetic, `docs/known-issues.md`.
- Gate: `make test-crate C=plx_plex`, `C=plx_data`, `C=plx_media`, `make test-hostsim C=plx_media`.

### U6 - a version without a playable Part is offered and plays an empty Part key

**Invariant this fix must keep (our approved B4, reintroduced by the first draft and caught by the
challenger): the Default is the `Media` at WIRE index 0, never "the first row".** Dropping a Part-less
`Media[0]` makes `Media[1]` the first listed row; it is not the Default and a bare Play does not take it.

- `MediaVersion` gains `index: u32`, the entry's position in `Media[]` before any row is dropped
  (`#[serde(default)]`; `versions` is outside the canonical hash and recorded `Detail`s of
  single-version items serialize as before). Nothing downstream may use a list position as identity or
  as "Default": identity is `id`, the Default is `index == 0`.
- Tests (red first): data (`pms.rs`) `versions_of` drops an entry with no `Part` or an empty
  `Part[0].key`, keeps each survivor's wire `index`, and returns nothing below two playable entries;
  **a three-`Media` fixture with a Part-less `Media[0]` and two playable others** yields rows with
  `index` 1 and 2 (no row has `index` 0); data (`metadata.rs`) `playing_version_from` for a
  listed-but-Part-less id answers `Gone`, not `part: ""`. Media/app tests for that fixture are in N3/N4:
  no row is ticked, a bare Play still takes `Media[0]` (refused as today when nothing is remembered), a
  pick of `Media[1]` plays `mediaIndex=1` and IS remembered (its `media_index` is 1, not 0).
- Fix: a `Media` is a row only if `Part[0].key` is non-empty; `playing_version_from` filters the picked
  entry the same way. Fewer than two playable rows means no list (as today for a Part-less default
  with one playable second version: refused as today, in `docs/known-issues.md`). Mock fixture: the
  Part-less-`Media[0]` movie of N6 gets a **second** playable version (two playable others).
- Gate: `make test-crate C=plx_data`, `ci/check-deps.sh` (`pms.rs`/`metadata.rs` are scanned files).

### U7 - the list's fit is untested for its worst row

- Test (appkit `version_list.rs` tests or screens): extend `the_list_fits_in_every_language` with a
  fixture whose four differing facts (video + HDR, audio + layout, container, size) are all at their
  longest (Dolby Digital Plus 5.1, MPEG-4, AVI, DTS-HD MA, a three-digit-GB size), in `item_menu` and
  `versions_panel`, across `i18n::SHIPPED`, asserting `menu_cap_failure`, `app_fit_failures` and
  `app_fit_failures_hugged` are empty. Today's fixture (`sample()`) shares audio and container, so the
  sub-line never carries them. **Measure first; red or green decides the step.** If red, the fix is
  token-priority truncation (drop the lowest-priority fact first: size, container, audio, never the
  rate/resolution label), our `join_within` idea, inside `version_list::labels` only.
- Gate: `make test-crate C=plx_appkit`, `C=plx_screens`.

### U8 - `dca-ma` reads "DCA-MA"

- Test: `friendly_codec("dca-ma") == "DTS-HD MA"`, and that the Track information sheet and the list agree
  (both read `friendly_codec`). Fix: one arm in `data/src/metadata.rs:friendly_codec` (our R10, one map).
  Gate: `make test-crate C=plx_data`, `C=plx_appkit`.

Minor, test-only (S3): `playing_version_from` with a failed fetch logs "gone" and plays the default
silently (see N3 for why the memory must tell the two apart); track memory matches across the versions
of one episode (descriptor, not `Stream.id`; port of `1dfd7f83`, now phrased with `MediaId`).

## 4. The feature delta upstream does not have

### N1 - `version_memory` preference list (port of our steps 4 and the batch-A cleanup)

Follows "the contract for a new setting" in `rust-modules/plex/src/plex/CLAUDE.md`. Our commits
`31628fa9` (+ `9a28735c`) apply to `origin/main` without a code conflict (checked with
`git merge-tree`; only `docs/version-selection-plan.md` conflicts, which is not carried):
`Preferences.version_memory: Vec<ProfileList<VersionMemoryEntry>>` (`de_soft_vec`, skipped when empty),
`session/prefs/version_memory.rs` (entry `{machine_id, rating_key, media_id}`, `CAP 128`,
`BUDGET_BYTES 24 KiB`, account-bound, `forgets()` for `media_id <= 0`), `VersionMemoryPref: Row`
(`SCOPE PerItem`, `LIVE false`, `SETTINGS None`, `PARSE SoftVec`), `DESCRIPTORS` line, shed **last** in
`prefs::shed_memory` (after `library_sorts`), `platform/storage/state.rs:ACCOUNT_BOUND_PREFERENCES`
(+ its two tests), `ci/list-caps.ini` sections `[plex/src/plex/session/prefs/version_memory.rs::CAP]`
and `::BUDGET_BYTES` (class `work`), `plex/CLAUDE.md` inventory and shed-order prose. The persisted
`media_id` stays an `i64` (the wire entry requires a JSON integer, like `track_memory`); the API above it
uses `MediaId`.

### N2 - the media door (port of step 5)

`route/prefs.rs` (`VERSION_MEMORY: MemoryMirror`, `impl MemoryTenant`, third `MEMORY_ROWS` entry),
`route/decision_memory.rs`: `recalled_version(sid, rk) -> Option<MediaId>` and
`remember_version(sid, rk, Option<MediaId>) -> bool` (`Some` remembers, `None` forgets, `false` for an
unseated boot, an unrestored mirror, an unregistered server). Both are main-thread only, over
`memory_scope` + the door's `recall`/`remember`; neither file is scanned by the `globals` gate.

### N3 - recall once, settle at the landing

1. **Recall (our B1).** In `request_play_inner`, before `s.request = Some(request.clone())`:
   `request.version = request.version.or_else(|| recalled_version(sid, rk))`, only when
   `retry.is_none() && !request.preview && !context_omits_queue_continuous(ctx)` (a fresh play of a
   film or episode; trailers, extras and previews never read it). From then on `ps.request` carries the
   identity, so the two re-entries inherit it. `request_env` only copies `request.version` (U1 removes
   its zeroing).
2. **Settle at a successful landing.** In `pump_play`, after `apply_plan` and only when the plan is
   playable and `request.version` is `Some`, with the lookup outcome the resolve produced:
   - picked and `media_index != 0` -> `remember_version(sid, rk, Some(id))`;
   - picked and `media_index == 0` (wire index 0, the Default), or the item was fetched and no longer lists
     the id (or lists it Part-less) -> `remember_version(sid, rk, None)`. This is "a pick of the Default
     (index 0) forgets", the stale drop, and "a rescan made the remembered id `Media[0]`" in one rule;
     it keys on the wire index, never on a row's position in the list;
   - the item could **not** be fetched -> touch nothing. Therefore `fetch_playing_item`'s `Option<PlayedVersion>`
     (against its #1013 signature, `(client, sid, rk, want)`) becomes a small enum `VersionLookup { Default, Picked(PlayedVersion), Gone, Unanswered }` (two
     production call sites in `plan.rs` plus tests, one signature change on an upstream API; without it a network blip would erase a pick).
   The door is called only when the stored value would change (it queues a storage write per call).
3. Explicit beats memory: an explicit `Some` never consults the memory (the row at wire index 0 is an
   explicit pick of the Default, so it overrides and then forgets). With a Part-less `Media[0]` no row has
   index 0: the Default cannot play at all, so there is nothing to go back to and a remembered pick of
   `Media[1]` simply stays (listed in `docs/known-issues.md`).
4. A refused or failed plan writes nothing (a pick that cannot play is not remembered).
5. Unseated boot, `--guest`, dev token: the door is a no-op; the explicit pick still plays (it rides the
   request). The device memory legs therefore use `tools/tv-session.sh up --mock-plextv=PORT --screen login`.

Tests (media, `decision_version_tests.rs`/`decision_version_memory_tests.rs`, the first two red-first
against "memory only for `fresh_play`", exactly as on our branch):
`a_remembered_version_survives_the_contract_changed_in_flight_re_resolve` (bump
`desired_contract_revision()` before `pump_play`, assert the second resolve asks `mediaIndex=1`);
`a_retry_after_a_refused_first_plan_keeps_the_version` (`Plan::default()` then `retry_current_play`);
`the_explicit_pick_wins_over_a_different_remembered_one`; `a_pick_of_wire_index_0_forgets`;
`with_a_partless_media_0_and_two_playable_others_a_pick_of_media_1_is_remembered`;
`an_up_next_successor_recalls_its_own_episodes_pick`; `a_retry_landing_never_settles`;
`an_extra_a_trailer_and_a_preview_landing_never_settle`;
`a_refused_plan_after_a_remembered_pick_leaves_the_old_pick_standing`;
`a_vanished_remembered_id_is_forgotten_and_the_default_plays`; `a_failed_fetch_keeps_the_pick`;
`trailers_extras_and_previews_never_recall`; `an_unseated_boot_reads_and_writes_nothing`;
`two_servers_with_the_same_rating_key_do_not_collide`; `a_seated_profile_with_no_pick_plays_byte_for_byte_as_before`
(the golden: recall answers `None`, the request is unchanged).

### N4 - a tick on "the version Play takes" (small, no shape move)

Upstream rows are "play this now" buttons in server order. Once Play can take a remembered version, the
list must say which. `MediaVersion` gains `plays: bool` (`#[serde(default, skip_serializing_if = "is_false")]`,
excluded from the canonical hash like `versions` already is in `pms/initial.rs`), documented as "the
version a bare Play takes for this profile". The rule is ONE pure function in `plx_data::pms`,
`mark_plays(&mut [MediaVersion], recalled: Option<MediaId>)`, used by every site below: **the row whose `id`
is the recalled one if it is listed, else the row whose wire `index` is 0 if it is listed, else no row**
(never "the first row": with a Part-less `Media[0]` the first row is `Media[1]`, which a bare Play does
not take). With no row ticked the cursor opens on the first row. Tests (data + screens): remembered id
listed; remembered id gone -> the index-0 row; index-0 row dropped (Part-less `Media[0]`, two playable
others) -> no tick, and a remembered `Media[1]` is ticked. It is set by the
application, never by the data layer (data cannot name the memory): at the menu captures
(`bridge.rs` `strip_menu_arg` where the episode menu clones `ep.versions`, the card menu builder `card_menu_arg`
(`ItemMenuKind::Card`, `bridge.rs:~3111`), the watchlist's matched card in `watchlist.rs`, and the dev bench menu at
`dev/scenarios.rs:~2604`, which builds an empty `PmsMovie` so nothing is marked there; named so a grep for
`ItemMenuKind::Card` finds every site) on the cloned versions, and for the hero panel
by one `MetadataCmd` (`MarkPlaysVersion { sid, rk, media }`, applied to `Detail.versions` or the
matching `Detail.episodes[..].versions`, issued by the app just before it presents
`ContentPanel::Versions`). `version_list::section` draws `Row::checked(v.plays)` and
`VersionsPanelScreen::new` / `build_versions` open the cursor on that row. Computing it at open time
(not at landing) means no hook and no staleness after a pick. The controlled encoding of the new
command is the existing "unsupported controlled metadata command" arm (recorded flows never emit it).
**Design target: no `SHAPE` string changes** (`VersionsPanel{.. versions:[i64] ..}`,
`item_menu::SHAPE` and the screen census stay; `the_screen_shape_inventory_is_pinned` stays green with
the pin untouched). If an implementation cannot hold that, it bumps `SCREEN_SHAPES_PIN` in the same
commit and S16 applies.

### N5 - harness `clear` arm (port from `d6f8b67b`, hunks only)

`plxnative-versionmemory=clear` beside `plxnative-trackmemory=clear`: `dev/scenarios.rs` (the generic
`forget_all_memory::<E>` and `arm_version_memory`; the seat watch calls it), `tests/run.py` (written for
every case), `tests/test_harness.py` (the assertions and `test_every_case_forgets_version_memory_at_the_seat`),
`tests/README.md` (one paragraph). Without it one seated device case leaks its pick into the next.

### N6 - mock reconciliation

Extend upstream's `--multi-version` (the flag name, its two items and ids from 9_900_000 stay; default
off keeps every generated byte). Port from our `f3faef59`, hunk by hunk, only what upstream lacks:
`mediaIndex` handling in `decision` and `start` (out of range -> the server's refusal shape, HTTP 400),
`--version-decision chosen|all|all-unflagged|ignore` (our `all-no-ids` becomes `all-unflagged`, the
unit being the `selected` flag now), and the extra items in our reserved block (940001+): a 9-version
movie with one stacked version at index 2, a two-version episode beside an unversioned one, a movie
whose `Media[0]` has no `Part`, a movie whose `Media[0]` is stacked. Media ids unique across the
library and never equal to a ratingKey; every Part key contains its Part id. The flag adds attributes to
the library only when on. Tests: `tests/test_mock_pms_library.py` (class `MultiVersion`: counts,
uniqueness, alphabet, default-off byte equality, the four decision modes),
`python3 tests/mock_pms.py --selftest`, `python3 tests/test_harness.py`; `tests/README.md` merges the two
flag paragraphs. `--version-fixture` is not introduced.

### N7 - optional: the hero chips follow the remembered version

Open decision O2. Not part of S1-S11.

## 5. Porting matrix (all 32 commits of `origin/main..version-selection`)

DROP = upstream covers it or the surface is gone. PORT = apply or adapt (`git cherry-pick -n <c>` then
fix; conflicts predicted with `git merge-tree`). NEW = designed here.

| commit | what | verdict | how |
|---|---|---|---|
| c3d6be15, 33fba14d, 9e2cea3d, 6efbcfd6, 7e43c03a | parity-spec ancestry (`parity-specs` branch) | DROP | other features' specs; the version spec file is taken from c3d6be15..43dc7cb2 state, see S0 |
| a9db9f68 | plan, challenge, spec corrections | DROP (history) | stays on branch `version-selection`; only the amended spec lands (O4) |
| 43dc7cb2 | step 0: F1-F14 into `docs/pms-api.md`, spec amended | PORT (docs) | S0; adapt wording (`MediaId`, no plan-section references) |
| ac28dfd7 | step 1: goldens of default `transcode_query`/`mde_decision`/`build_stream` bytes | PORT-adapt, optional | S2; mechanical (new `media_index` argument); conflict only in the `mod` line of `decision.rs` |
| 72757c3c | step 2: `Media.id`, `VersionPick`, `HdrKind`, `resolve_pick`, `narrowed_to` | DROP, except `Media.duration` | `MediaId`/`media_version` replace it; `Media.duration` -> U5; `narrowed_to` redone index-only -> U4 |
| f58f061b | step 3: version on the wire + narrowing door | DROP (wire), PORT-adapt (door) | wire is upstream's; door -> U4 (tests re-expressed with `selected`) |
| 31628fa9 | step 4: `version_memory` list | PORT | N1; code applies cleanly; drop the plan-doc hunks |
| 463a26cb | step 5: media door | PORT | N2; `i64` -> `MediaId` at the API |
| ac0f3508 | spec leniency line | DROP | |
| a269cc8a, c6db7187, b0fcce07, f2a25fa1, be3b1c30 | reviewer verdict files | DROP (history) | stay on the branch |
| dcd61eac | step 6: `Detail.versions/version`, `select_version`, `carry_version`, `SelectVersion`, `PlayingItem.source`, `subtitle_rows(part_id)`, `resume_ns_for` | SPLIT | PORT only `subsearch.rs` (U3) and `resume_ns_for` (U5); DROP the rest (no selection state to carry) |
| 256516ce | step 7: resolve follows the version; B1, B2, B3, resume clamp, request rebase, 760-line test file | SPLIT | B2: DROP (upstream). B1: NEW-on-upstream (N3); take the two re-entry tests from its test file. B3: DEFECT (U1, redesigned: measured on the worker). Resume clamp: PORT (U5). Request rebase: DROP, replaced by `cur_part` (U2). Narrowing door: U4 |
| 9a28735c | batch-A cleanups (prefs prose, size line, worst-case test) | PORT | squash into N1 |
| 392897c5 | subtitle search scope + `wants_open` | PORT-adapt | U3; conflict only in `overlay.rs`; scope source is `cur_part_id(ps)` |
| 6011723b | side reader names the played Part | PORT-adapt (test) | U2; the fix differs (`cur_part`) |
| 1dfd7f83 | track memory matches across versions | PORT-adapt (test only) | S3 |
| ca497cdd | step 9: three i18n keys x 8 locales | DROP | upstream's keys already exist in all 8 locale files (`browse.detail.play_version`, `browse.menu.play_version`, `browse.menu.play_version_title`, 3 each); N1-N7 add no string |
| b135ef40 | step 8: label composer, 7th hero slot, `fmt_layout` hoist, `friendly_codec` dca-ma | DROP, except dca-ma | U8 only |
| 25c42b0e | step 10: Versions pill, `version_menu`, registry, `alt_sources`->`ui::popover::panel_below`, `ctl_pop` fix | DROP | upstream has the disc, panel, `under_or_above`, and already uses `CtlPop<{ MAX_CTLS }>`; the `tests/manifest.json` scene `versions-panel` may be re-pointed at upstream's panel (optional, S15) |
| d6f8b67b | step 11: pick handler, landing hook, memory wiring, clear arm | SPLIT | PORT the harness hunks (N5) and the idea of "door only when the value changes" (N3); DROP `version_pick.rs`, `picked_version`, the landing hook (settle moved to `pump_play`, N3) |
| f3faef59 | step 13: `--version-fixture`, `mediaIndex`, decision modes | PORT-adapt | N6, merged into `--multi-version` |
| 89c04c39 | alphabet admits "Versions" | DROP | no new overlay word |
| febc9253, f9eea137 | batch-D notes, replay re-record | DROP | S16 re-records only if a shape moved, once |

Predicted textual conflicts of the PORT rows (merge-tree against `origin/main`): clean for `31628fa9`,
`463a26cb` (code), `9a28735c`, `6011723b`, `1dfd7f83`; one file for `ac28dfd7` (`decision.rs`),
`392897c5` (`overlay.rs`), `f3faef59` (`tests/mock_pms.py`). The big files (`decision.rs`, `plan.rs`,
`metadata.rs`, `hero.rs`, `registry.rs`) are exactly where upstream keeps moving: keep commits small,
merge `origin/main` into the branch before every step's gate.

## 6. Implementation order

Rules for all steps: one commit each (the branch may carry more; `main` takes the one squash). Gates run
on the COMMITTED tree with `git status --short` empty before and after; report the `test result:` line,
not a summary. Every direct cargo call carries `CARGO_INCREMENTAL=0`; `make` from a non-login shell needs
`PATH="$HOME/.cargo/bin:$PATH"`. `make check` is queued machine-wide (two slots): run it in the
background, never in the foreground with a short timeout. After editing `rust-modules/**/src/**/*.rs`
also `make check-nodef` when the release hook did not run, and `make lint`. Layering: references only
point down (`ci/module-layers.ini`); nothing new outside `[port webos]` names `webos`, `keymanager`,
`system`, `player::ffi` or `port`.

| step | content | gate (besides `make lint`, `make check-nodef`) |
|---|---|---|
| S0 | docs only: the amended spec with the status header (section 7), F1-F14 into `docs/pms-api.md` (and the "must iterate" sentence there, now true), `docs/known-issues.md` entries | `make check` |
| S1 | merge `origin/main` (this plan is written against #1013's `fetch_playing_item(client, ..)`); `make test-crate C=plx_media T=version_tests` green as the baseline | the `test result:` line |
| S2 | optional goldens of the default bytes (`ac28dfd7` adapted) | `make test-crate C=plx_plex T=transcoder`, `C=plx_media T=version` |
| S3 | characterisation tests only: re-entry (`pump_play` contract change, `retry_current_play`) keeps the version, retranscode fallback uses the picked file's codecs, track memory across versions of an episode | `make test-crate C=plx_media` |
| S4 | **U1** quality source rate (data + media) | `C=plx_data`, `C=plx_media`, `make test-hostsim C=plx_media`, `ci/check-deps.sh` |
| S5 | **U2** `cur_part` / side reader | `C=plx_media`, `make test-hostsim C=plx_media`, `ci/check-deps.sh` |
| S6 | **U3** subtitle scope (data + overlay) | `C=plx_data T=subsearch`, `C=plx_screens`, `make lint`, `ci/check-deps.sh` |
| S7 | **U6** unplayable rows, **U8** `dca-ma`, **U7** worst-row fit test (fix only if red) | `C=plx_data`, `C=plx_appkit`, `C=plx_screens`, `ci/check-deps.sh` |
| S8 | **U5** resume clamp (`Media.duration`, `PlayedVersion.dur_ms`, `pump_play`) and **U4** the `/decision` door (two commits; U4 only if O3 = yes) | `C=plx_plex`, `C=plx_data`, `C=plx_media`, `make test-hostsim C=plx_media`, `ci/check-deps.sh` |
| S9 | **N1** `version_memory` prefs + platform + caps + CLAUDE.md | `C=plx_plex`, `C=plx_platform`, `python3 ci/check-list-caps.py`, `python3 ci/check-module-layers.py`, `ci/check-deps.sh` (ledgers unchanged) |
| S10 | **N2** media door, then **N3** recall + settle + `VersionLookup` (two commits; N3 needs S4) | `C=plx_media`, `make test-hostsim C=plx_media`, `make test-fast`, `ci/check-deps.sh` |
| S11 | **N4** `plays` tick + `MarkPlaysVersion` + the menu captures, then **N5** clear arm | `C=plx_data`, `C=plx_appkit`, `C=plx_screens`, `make test-fast`, `make test-hostsim`, `python3 tests/test_harness.py`, `ci/check-deps.sh` |
| S12 | **N6** mock reconciliation | `python3 tests/test_mock_pms_library.py`, `python3 tests/mock_pms.py --selftest`, `python3 tests/test_harness.py` |
| S13 | docs: README bullet, doc-claim-auditor pass over the targets below, `docs/parity-gaps.md` | `make check` |
| S14 | the merged-tree gate: `make check` (if it is red, the only expected cause is `ci/test_disk_preflight.py`, a free-space assertion about this machine's disk, not a code failure; report it as such), `make test-hostsim`, `make test-hostsim C=plx_media`, `make check-nodef` | the `test result:` lines |
| S15 | verification: simulator (below), device legs under `tv-lock` against the mock | which-tier, tv-lock |
| S16 | **conditional**: if (and only if) a step moved a screen `SHAPE`, `SCREEN_SHAPES_PIN` was bumped in that same commit and the three anchors are re-recorded once, here, with the procedure in `tests/fixtures/replay/README.md` (`tools/plxnative-rec rerecord`, never by hand), then `make check-replay` | `make check-replay`, `make check` |

Notes on the table:

- S4-S8 are independent of each other and of S9; the merge-risk order is S4 (`plan.rs`, `decision.rs`),
  S5, S8, then the rest.
- **Ledgers (`ci/allow/**`) must not grow.** `workers`, `globals`, `forks`: the memory reads live in
  `route/decision_memory.rs` and `decision.rs`/`src/app` (not scanned); `subsearch.rs` adds a
  parameter only. Existing keys are `<path>::<fn>::<spelling>`: do not rename or split `build_stream`,
  `fetch_detail`, `fetch_playing_item`, `cached_playing`. #1013 took the `globals` pin down by eight
  (`client_for` lines left `ci/allow/globals/data-metadata.txt`) and #1015 moved the totals again; the pins
  at `fba7a811` (`tests/test_harness.py:PINNED_LEDGER_TOTALS`, workers/globals/forks/fxlanes) are 16/78/58/11,
  and the ledgers on disk sum to exactly that; the totals only shrink, and `fetch_playing_item` now takes the client, so no new registry read is needed
  in the data chain. `fxlanes`: no new `AppFx` arm (the new
  command is a `MetadataCmd`). `layers`: `plays` is set by the app (top layer) into a data type.
- **Caps (`ci/list-caps.ini`):** only the two `work` sections of N1; no `MAX_*`/`*_MAX_*`, no `reach`
  entry. A U7 fix that clamps tokens by character needs one `not-a-list` section for the symbol (our
  `version_label.rs::elide` precedent).
- **`ci/plex-requests.ini`:** no new request site.
- **i18n:** no new key. If O-decisions ever add a word, `en` with `value` + `description` and all of
  `be de es fr hu it pt` (`locales/languages.json` lists exactly these eight) plus a fit test per
  `i18n::SHIPPED`.
- **Simulator (`ui-sim`, S15):** `python3 tests/mock_pms.py --port P --multi-version --media DIR`;
  captures at `SIM_W=1920 SIM_H=1080` (larger drawables abort the simulator's own screenshot encode,
  pre-existing, see batch-D review remark 6): the hold-menu page and the hero panel with the tick on
  the remembered row, the nine-version item (scroll), the U7 worst row in `de|be`. The simulator cannot
  grade frame rate, LG text rasterization or the video plane.
- **Device (S15):** `tv-lock` first, one device command per call, boot `--guest --mock` against the mock
  (never the owner's server; a play writes timeline/scrobble state to a real household record). Legs:
  direct play of version 0 and 1 (log lines differ), the `start` leg with `mediaIndex=1` (force a remux by
  quality), a fixed rung with a 1080p picked version direct-plays (U1), seek/audio switch keep the
  version, the resume clamp (U5). The memory legs (pick -> relaunch -> Play takes it -> the index-0 row
  forgets) need a seated profile: `tools/tv-session.sh up --mock-plextv=PORT --screen login`. Never press
  `ok` on a card whose `ratingKey` was not read from the log.
- `fw-compat-reviewer` is not needed (no FFI/linkage change); `doc-claim-auditor` is S13.

## 7. Documentation

- **`docs/version-selection-spec.md`** lands with a final header line, replacing "Status: proposed,
  nothing built":
  `Status: implemented upstream (#955 Play Version) + delta (docs/version-delta-plan.md); sections 3, 4.1
  and the label of section 3 are superseded by #955's list; 4.2, 4.3, 5-9 stand as amended below.`
  and each superseded section gets a one-line "superseded by #955: <symbol>" pointer rather than a
  rewrite. Also fix the leftovers our reviewers listed: the `MAX_VERSIONS` truncation test, "a version
  without a `Part` (dropped)" (now: unplayable rows dropped), "popover opens from the hold menu"
  (it does, as upstream's page), the field names.
- **README.md "What it does"**, one bullet after "Everything you expect while watching":
  `- **Choose the version to play** when a title has several (a 4K and a 1080p copy, MKV or MP4): the
  hold menu and the detail page list them, and the one you pick is remembered per title for each
  profile; a title with one version looks exactly as before.`
- **`docs/pms-api.md`**: F1-F14 (counts only), and the sentence at "The picker must iterate `Media[]`"
  now true; keep `mediaIndex` semantics marked UNVERIFIED.
- **`docs/known-issues.md`**: stacked versions play `Part[0]` (also for a single-version stacked item
  today); a Part-less `Media[0]` stays refused; the Info card runtime on a picked play; HDR is absent
  from card-menu rows (listing rows carry no `Stream[]`); `mediaIndex`/`selected` behaviour on the real
  PMS is mock-verified only.
- **doc-claim-auditor targets** (contradictions only): `decision.rs` docs of `PlaybackRequest.version`
  ("`None` for the default" becomes "None = the remembered, else the default"), `request_env`'s comment
  "Both describe the default version", `request_play`'s doc; `plan.rs:ResolveEnv.version` doc;
  `pms.rs:versions_of`/`MediaVersion.hdr`/`plays` docs; `metadata.rs` `Detail.versions` doc and the
  "PRIMARY version" blocks; `rust-modules/plex/src/plex/CLAUDE.md` (preference inventory, the
  `ACCOUNT_BOUND_PREFERENCES` description, the document-guard shed order); `docs/parity-gaps.md` (the three
  LANDED entries: "next-episode autoplay plays the default", "Still open: the device has not played..."
  and lines 184-185); `docs/architecture-review-2026-07-26.md:327-330`; `docs/pms-api.md` 818-819 and 976;
  `tests/README.md` (mock flags); `docs/version-selection-spec.md` itself. `AGENTS.md` and
  `docs/agent-reference.md` mention none. Not an audit target: `docs/plex-api.md`, `plex-api-catalog.md`
  (they describe `mediaIndex` only as a query parameter).

## 8. Open decisions and risks

### Decisions for the owner

| id | question | recommendation |
|---|---|---|
| O1 | **SETTLED (coordinator): sticky per profile.** **Sticky pick.** Upstream's rows mean "play this now". Our approved behaviour (spec 4.3) remembers every pick except the Default (wire index 0) per profile, and a pick of index 0 forgets. Keep it sticky, or remember only through a separate action? | sticky, as approved; the tick (N4) makes the state visible |
| O2 | **SETTLED (coordinator): option A, documented in `docs/known-issues.md`.** See Challenge responses for one ambiguity. **Do the hero chips, About and Track information follow the remembered version?** They read `Detail`'s flat fields, which are version 0's. With a remembered pick the page shows 4K while 1080p plays (our principle 3). (A) accept and document it; the tick says what Play takes. (B) project the remembered version at fetch time: `fetch_detail` takes `want: Option<MediaId>` (hint stored in `MetadataState` by the app before `Load`, from `recalled_version`) and feeds `parse_streams`, `part`, `vcodec`, `acodec`, `dur_ms` from `media_version(want)`; about 80 lines plus the hint command, one more step. Not our heavy `select_version` machinery: there is no in-page selection to change. | A in this change, B as a follow-up step after S15 if the owner dislikes the mismatch |
| O3 | **SETTLED: yes, index-only, mock-only, active only for `index != 0`.** **The `/decision` door (U4).** Upstream relies on the `selected` flag, which no read-only probe could confirm for a decision body (F6 only says the library listing has none). The door is ~60 lines + tests and makes every outcome safe; it still proves nothing about the real PMS. | yes, index-only, mock-verified, labelled as such |
| O4 | **SETTLED: as proposed, plus: tag the branch (`git tag version-selection-final version-selection`) before anything is dropped.** **Which documents land.** Only the amended spec (with the status header) and the F1-F14 facts; keep plan, challenge and the four review files on branch `version-selection` (tag it before deleting anything). | as stated |
| O5 | **SETTLED: drop, with the wire-index-0 Default rule of U6/N4.** Unplayable-version policy (U6): drop the row (recommended) or show it disabled. A disabled row needs a new state in `version_list`. | drop |
| O6 | **PENDING owner (TV time).** U1, U2 and U5 are host/mock-verified only and must not be called device-verified. Device verification of a picked version has never happened (upstream's own caveat). Schedule the mock-backed TV leg (S15) before the squash, or accept "simulator + host only" in the PR text. | schedule it; the memory legs need the seated `--mock-plextv` boot |

### Risks

| id | risk | mitigation |
|---|---|---|
| R1 | Recall in `request_play_inner` touches every play path (card, hero, episode, Up Next, links). | the golden `a_seated_profile_with_no_pick_plays_byte_for_byte_as_before`; retries/previews/extras excluded by the same condition as our B1 |
| R2 | A network blip erasing a pick (the ambiguity in `playing_version_from`). | `VersionLookup` (N3.2) and the test `a_failed_fetch_keeps_the_pick` |
| R3 | Merge conflicts: upstream keeps changing `decision.rs`, `plan.rs`, `hero.rs`, `registry.rs`. | small commits, merge `origin/main` before each gate, apply the predicted-conflict list of section 5 |
| R4 | A step moves a screen shape after all (the tick). | design target: none; `the_screen_shape_inventory_is_pinned` is the guard; then bump in the same commit and S16 once |
| R5 | Recorded `Detail`/`PmsMovie` bytes change. | `plays` is skipped when false and outside the canonical hash; `record::validate` tests of a single-version item stay byte-identical |
| R6 | U7 is red: the worst row overflows the 650 px menu cap in some locale. | token-priority truncation inside `version_list::labels` (S7) |
| R7 | Real-PMS behaviour for `mediaIndex=N` / `selected` differs from every modelled answer. | by decision D10 nothing is claimed; PR text says "mock-verified"; the door degrades to the existing "unusable decision" path |
| R8 | `fetch_playing_item`'s return type changes (2 production call sites + tests, written against #1013's signature). | compile-driven; the only upstream API change in the plan |
| R9 | The memory list is the fourth account-bound list: a profile with all four near their caps pushes the document guard. | N1 ports the order test (shed last) and the worst-case household test |

## 9. Challenge responses (round 1: `docs/version-delta-challenge.md`, REJECT, one blocking item)

I re-derived each finding in code; none is disputed.

- **Blocking: U6 vs "first row = Default" - confirmed and fixed.** `pms.rs:versions_of` maps entries in
  order, so once Part-less rows are dropped the first row can be `Media[1]`; a "remembered, else the first"
  tick and a "first row forgets" test would then contradict the settle rule, which (correctly) keys on
  `media_index`. Changes: `MediaVersion.index` (wire position); the Default is the row at wire index 0
  everywhere ("index 0 forgets"; the words "first row" survive only for cursor placement); `mark_plays`
  = recalled id if listed, else the index-0 row if listed, else no row; the fixture and tests with a
  Part-less `Media[0]` and two playable others are in U6, N3 and N4, and the N6 mock fixture has two
  playable others. Consequence stated: with a Part-less `Media[0]` there is no way back to the Default
  (it cannot play), so a remembered pick stays; known-issues entry.
- **Non-blocking:** (1) base moved: fast-forwarded to `fba7a811`; `fetch_playing_item(client, sid, rk,
  want)` is #1013's signature, `VersionLookup` is written against it, the ledger note and totals
  updated (corrected in batch 1: the pins are 16/78/58/11 after #1015, which moved the totals but touches none
  of the version code). (2) `ci/check-deps.sh` added to S4-S8,
  S10, S11; the `make check` red item is phrased as the environmental free-space assertion. (3) N3 tests
  added: Up Next recalls, retry and extra/trailer/preview landings never settle, a refused plan leaves the
  old pick. (4) replay "none expected": agreed, no change. (5) `dev/scenarios.rs:~2604` added to the capture
  list (it builds an empty `PmsMovie`, so nothing to mark). (6), (7) agreed, no change. O4's tag added.
- **Unresolved:** the coordinator's O2 wording ("chips follow the picked/remembered version only at
  fetch time") reads like my option B (fetch-time projection), not A (chips stay version 0). The plan
  keeps A as written; if B is meant, it is one extra step after S15 (O2 row).

## Implementation notes, batch 1 (S0-S5)

Deviations and findings, appended to as each step lands.

- **S0.** The amended spec is a full rewrite-by-pointer of `docs/version-selection-spec.md` taken from the
  tip of `version-selection` (it did not exist on `origin/main`); the dangling references to
  `docs/parity-specs.md`, `docs/playlists-spec.md` and the `version-selection-plan.md` section numbers were
  replaced by pointers into this plan. Tag `version-selection-final` was created on `version-selection`
  (O4); `version-selection-archive` already pointed at the same commit. Ledger pins corrected in sections 6
  and 9 above (16/78/58/11, verified against `tests/test_harness.py:PINNED_LEDGER_TOTALS` and the sums of
  `ci/allow/*/*.txt`). `docs/known-issues.md` gained one section with the entries the plan lists (stacked
  `Part[0]`, Part-less `Media[0]` stays refused, Info card runtime, HDR absent from card rows, mock-only
  `mediaIndex`/`selected`) plus the two the challenger found missing (O2: Detail page facts stay on version
  0; a Part-less `Media[0]` has no way back to the Default) and the one-playable-other reach loss
  (challenger Round 2 remark 3, recorded as accepted for the owner). F6 in `docs/pms-api.md` is worded as
  "no `Media.selected` in the listing" (the plan's section 8 O3 says the listing is all F6 measured).
  The new docs cite `docs/version-delta-plan.md`, which lives on this branch only; whoever squashes decides
  whether it lands (plan O4 says plan and challenge files stay on the branch), and the three citations in
  `docs/pms-api.md`, `docs/version-selection-spec.md` and `docs/known-issues.md` then need repointing.
- **S1.** Nothing to commit: `origin/main` is `fba7a811`, an ancestor of this branch, so the merge is a no-op.
  Baseline on the clean tree: `make test-crate C=plx_media T=version_tests` gave `test result: ok. 5
  passed; 0 failed`. S0's `make check` aborts at its first failing branch (the known environmental
  `ci/test_disk_preflight.py`), which stops the cargo branch too, so the S0 gate was run as its two halves:
  `make check-python-harness` (`Ran 543 tests ... OK (skipped=1)`) and the 108-step python/shell branch
  (107 green, only `test_disk_preflight` red). The cargo half (`make check-cargo`) is run once on the
  batch's last commit.
- **S2.** Done, by `git cherry-pick -n ac28dfd7` (one conflict: the `mod` line in `decision.rs`) plus the
  new `media_index` argument at the two `mde_decision*` calls in the transcoder goldens. No golden bytes
  changed, i.e. upstream's `media_index` plumbing left the default transcript identical to our branch's.
- **S3.** Test-only, in `decision_version_tests.rs`: the loopback PMS became a persistent recording
  `VersionPms` plus a `World` (the one-shot `versions_pms`/`resolve` pair they replace is a wrapper now), and
  four characterisation tests were added: `a_pick_survives_the_contract_changed_in_flight_re_resolve`,
  `a_retry_after_a_refused_first_plan_keeps_the_pick`, `a_retranscode_uses_the_picked_files_codecs_in_every_fallback`
  (inputs are the claim inputs `src_vcodec/src_acodec/media_index` and `retranscode_fallback_codecs`) and
  `a_remembered_audio_track_matches_in_every_version_of_the_episode`. All pass on `origin/main` behaviour by
  design (they pin what upstream gets right). Non-vacuity was checked by mutation: with `request_env` forgetting
  `request.version`, the two re-entry tests and `the_picked_version_rides_the_request_and_the_landing` go red.
