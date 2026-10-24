# Version delta: challenger review, round 1

Reviewed `docs/version-delta-plan.md` (560 lines) against the tree at `4a551d0b` (worktree
`version-delta`), plus `origin/main` (now `ef3e55a4`, one commit ahead). Method: every "upstream covers
X" and every defect U1-U8 was re-derived by reading the code; `git merge-tree` re-run for the PORT rows;
`python3 -m unittest ci.test_disk_preflight` run once (1 failure: a free-space assertion about this
machine's disk, not a code failure). No real-server request was needed. No source edited.

VERDICT: REJECT (one blocking item, small).

## "Upstream already covers X": confirmed

- B1 explicit pick: `PlaybackRequest.version` is stored on `ps.request` (`decision.rs:PlaybackRequest`),
  `request_env` copies it, `pump_play` ("desired contract changed in flight") and `retry_current_play`
  re-enter with `ps.request.clone()`. True. The only test (`the_picked_version_rides_the_request_and_the_landing`)
  does not drive either re-entry, as the plan says.
- B2: `build_stream` reassigns `plan.src_vcodec/src_acodec` in the picked branch (`plan.rs` ~1620);
  asserted by `a_picked_version_direct_plays_its_own_part_after_asking_about_it`. True.
- Track store / part id / `mediaIndex` on MDE, decision and start; `Media.id` string and number parse;
  a vanished id plays the default: all pinned by `decision_version_tests.rs` and `models.rs` tests.
- The test harness `resolve()` builds its env with `ResolveEnv::snapshot`, not `request_env`, so U1's test
  (set `env.quality`, assert `plan.src_measure.0`) is feasible and red at HEAD (snapshot gives 0, the fix
  must produce the picked version's rate). `desired_contract_revision()` is already driven by
  `decision_resolve_route_tests.rs`, so the B1 re-entry tests are writable.

## Defects U1-U8: real, fixes sound

- U1 real: `request_env` sets `env.cached_item = None; env.src_kbps = 0` for every `Some` version;
  `Ceiling::admits` is `kbps > 0 && ..` and `quality_policy` denies direct play and remux on failure,
  `plan.src_measure = (env.src_kbps, ..)` keeps the 0. The fix (measure on the worker from the picked
  `Media`, video stream rate with the whole-file fallback, five `env.src_kbps` sites at `plan.rs`
  1935/2012/2286/2312/2323) is right and keeps one rule via `plx_data`.
- U2 real: `side_reader_target` returns `request.part`, the caller's (default) key; no other reader of
  `ps.request.part`. `Plan.part` + `cur_part` (not a request rebase) is the better fix.
- U3 real: `subtitle_rows` flat-maps every `Media`/`Part`; `overlay.rs` sends `Open` only when `!ours`.
- U4 real but unobservable on the real PMS: `decided_media()` is "flagged `selected`, else first"; the
  loopback harness exists, no test feeds a multi-entry body. Gated by O3, mock-only.
- U5 real: `pump_play` returns `ok.then_some(resume_ns)` unchanged and `arm_play_resume` is the single
  carrier (`PLAY_RESUME`, `requested_resume_ns`); the card path and the hero path both end there, so one
  clamp covers both.
- U6 real: `versions_of` maps every `Media` (no Part check); `playing_version_from` returns
  `part: ""` for a Part-less pick, which the picked branch shadows over the real argument.
- U7 plausible (the `sample()` fixture shares audio and container, so the worst sub-line is never
  measured); U8 trivial.

## BLOCKING

### 1. U6 breaks "first row = Default" that N3 and N4 build on (our approved B4 invariant, reintroduced)

U6 drops a `Media` without a playable `Part[0].key` from `versions_of`. With three `Media` where
`Media[0]` is Part-less and two others playable (fixture 940005 shape plus one), the list's first row is
`Media[1]`, not the Default. Then:

- N4 documents `plays` as "the remembered one, else the first": a bare Play takes `Media[0]` (refused,
  as today), the list ticks `Media[1]`.
- N3 says "the first row of the list is an explicit pick of the Default, so it overrides and then
  forgets" and tests `a_pick_of_the_first_row_forgets`: the first row there resolves to `media_index 1`,
  so the settle rule (keyed on `media_index`, correctly) REMEMBERS it. The prose, the tick and the test
  name contradict the rule.

Evidence: plan section 3 U6, section 4 N3.2/N3.3, N4; `pms.rs:versions_of`; `playing_version_from`.
Required change: define the default by wire position only. `plays` = the recalled id if listed, else the
row whose `Media` index is 0 if it is listed, else NO row; `MediaVersion` therefore needs the wire index
(or the app computes `plays` from the fetched `Metadata`); say "the row at index 0 forgets" instead of
"the first row"; add a fixture and a test with a Part-less `Media[0]` and two playable others (tick on no
row, a pick of `Media[1]` is remembered, a bare Play still takes `Media[0]`).

## Non-blocking remarks

1. Base is one commit behind: `ef3e55a4` (#1013) changes `fetch_playing_item` to take the client and
   removes 8 `client_for` lines from `ci/allow/globals/data-metadata.txt` (PINNED_LEDGER_TOTALS 90 -> 82).
   The plan's ledger note ("do not rename `fetch_playing_item` or the keys move") is stale; N3's
   `VersionLookup` change must be written against the new signature (S1's merge covers it, say so).
2. Gates: S4-S8, S10, S11 edit `MACHINES` files (`plan.rs`, `subsearch.rs`, `data/metadata.rs`) but list no
   `ci/check-deps.sh`; add it, otherwise ledger drift surfaces only at S14. `make check` known-red claim
   (`ci/test_disk_preflight.py`) is environmental (free space), not code; phrase it so.
3. N3 tests to add: Up Next successor recalls; a retry landing and an extra/trailer/preview landing
   never settle; a refused first row after a remembered pick leaves the old pick standing (state it).
4. Replay "none expected" verified: `ItemMenuArg` canon writes `row.kind` and `mark` only
   (`registry.rs:489-494`), `pms/initial.rs` ignores `versions`, `VersionsPanel` SHAPE has `sel`/`versions:[i64]`
   only, `SubSearchState` is not in the recorder shapes. `plays` and the cursor are values, not shape.
5. `MarkPlaysVersion`: three menu capture sites exist (`bridge.rs` `card_menu_arg`, `strip_menu_arg`,
   `watchlist.rs` matched card) plus `dev/scenarios.rs:2604`; the plan names the first three, add the
   fourth to the touch list.
6. i18n: no new key, all 8 locales already carry upstream's three; U8 changes an untranslated codec name.
   README bullet is consistent with O1 = sticky. list-caps: only the two `work` sections; correct.
7. N1 PORT rows verified: `31628fa9`, `9a28735c`, `463a26cb` merge-tree clean except the plan doc;
   `392897c5` conflicts only in `overlay.rs` as stated. `MEMORY_ROWS`/`shed_memory` at HEAD still hold
   exactly the two/three lists the commits assume.

## O1-O6

- O1 sticky pick: needs the owner (it changes upstream's one-shot rows into hidden state); engineering
  recommendation: sticky as the approved spec 4.3, with the tick visible.
- O2 hero chips: engineering settles A (document, tick says what Play takes); B only if the owner objects.
- O3 `/decision` door: yes, index-only, mock-verified, active only for `index != 0` (default path
  byte-identical); no owner input needed.
- O4 documents: as proposed; tag `version-selection` before any deletion.
- O5 unplayable rows: drop (settled), once blocking item 1 fixes what "default" means.
- O6 device leg: owner (TV time); engineering position: do not claim U1/U2/U5 verified without it.

---

# Round 2 (plan at `fba7a811`, 615 lines)

VERDICT: APPROVAL

## Blocking item 1 (U6 / Default): resolved

- Design: `MediaVersion.index` is the wire position before any drop; the Default is `index == 0`, never the
  first row; the settle rule keys on `media_index` (N3.2); `mark_plays` is one pure function in
  `plx_data::pms` (recalled id if listed, else the index-0 row if listed, else none) used at all capture
  sites plus `MarkPlaysVersion`.
- Tests that fail without it: the three-`Media` fixture (Part-less `Media[0]`, two playable) must give rows
  with `index` 1 and 2, no tick, and `with_a_partless_media_0_and_two_playable_others_a_pick_of_media_1_is_remembered`
  (a "first row forgets" implementation forgets it). The N6 mock fixture was changed to two playable others.
- "No way back to the Default when `Media[0]` is Part-less" is acceptable: the Default cannot play, so a
  remembered pick of `Media[1]` is the only playable choice; the entry is dropped by the stale rule if that
  id vanishes. It is stated in N3.3 and U6.

## Folded-in remarks: verified

Base fast-forwarded (`fetch_playing_item(client, sid, rk, want)` confirmed in `metadata.rs`); `ci/check-deps.sh`
in S4-S8, S10, S11; the Up Next / retry / extra / refused-plan tests are in N3; the `dev/scenarios.rs` capture
site is named; #1015 touches none of the version code (search store only).

## Non-blocking remarks

1. Ledger totals are stale after #1015: `tests/test_harness.py` now pins workers 16, globals 78, forks 58,
   fxlanes 11 (the plan says 18/82/58/11 and "#1015 touches none of this"; it moved the totals, not the
   version code). Fix the sentence at plan lines ~508 and ~608; the invariant "ledgers do not grow" is unaffected.
2. O2 = A is internally consistent (chips/About/Track information stay version 0; the tick says what Play
   takes; `plays`, `mark_plays` and the settle rule never read the chips; the resume clamp is at the landing),
   but the `docs/known-issues.md` list (plan ~550) does not yet contain the O2 entry or the "Part-less `Media[0]`
   has no way back" entry; add both to the S0 content.
3. U6 as written also removes a capability #955 has today: a Part-less `Media[0]` with exactly ONE playable
   other is listed now (the playable row plays), after U6 there is no list, so that version is unreachable.
   Rare and equal to pre-#955 behaviour, but it is a reach loss against upstream; either lower the threshold
   to "at least two `Media` and one playable row" (the surfaces gate on `versions.len() >= 2`, so it needs one
   `offers_versions()` predicate) or state it in known-issues and O5 for the owner.
