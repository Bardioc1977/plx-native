# Version delta: reviewer, batch 3 (S9-S11, `c1c61681..71dd4a8c`)

VERDICT: APPROVAL (no blocking issue).

## Own evidence

- **Gates (clean tree, HEAD):** plx_plex 748, plx_platform 281, plx_data 1223, plx_media 1777, plx_screens 1710,
  plx_appkit 350, `make test-hostsim` (app) 1121, `make test-hostsim C=plx_media` 1939, `make test-fast` all
  `test result: ok` (app crate 1007); `make lint`, `make check-nodef`, `ci/check-deps.sh` ("all gates green"),
  `check-module-layers.py`, `check-list-caps.py` (288 sites, all classed), `tests/test_harness.py` Ran 544 OK
  (skipped=1), `make check-replay` 3 fixtures, 6 replays, 0 failures (all SAME).
- **Mutation (recall off):** in `request_play_inner` the recall replaced by `None`. `make test-hostsim C=plx_media T=version`:
  7 FAILED, among them `a_remembered_version_survives_the_contract_changed_in_flight_re_resolve`,
  `a_retry_after_a_refused_first_plan_keeps_the_remembered_version`, `an_up_next_successor_recalls_...`,
  `a_vanished_remembered_id_...`. Restored with `git checkout`; tree identical to HEAD.
- **Persistence contract:** scope `PerItem` (a memory for content, like `subtitle_offsets`; correct per the
  contract), `ACCOUNT_BOUND` + `ACCOUNT_BOUND_PREFERENCES` + both sign-out tests, `de_soft_vec`, `skip_serializing_if`,
  `DESCRIPTORS` row, no key renamed, no `PublicPayload`/`Mutation` slot (diff touches `Preferences` only), previous-build
  round trip test (`tier1_the_real_version_memory_shape_rides_through_the_previous_build`), CAP/BUDGET_BYTES in
  `ci/list-caps.ini`, shed LAST (sweep test), worst-case household fills it, `document_size_line` reports all four.
  `ci/allow/**` untouched (only `ci/list-caps.ini` +8). No `SHAPE` string and no `SCREEN_SHAPES_PIN` change.
- **Recall/settle:** `continued` is `Some` only from `pump_play`'s contract re-entry; `retry_current_play` and
  `request_play` pass `None`; a retry has `fresh_feature == false`, so only the continuation of a settling landing
  settles. Trailers/extras (`context_omits_queue_continuous`) and previews neither recall nor settle. Up Next recalls
  its episode's id and can only "settle" by the stale drop (a recalled id yields no new pick).
  Settle is keyed on the WIRE index (`media_version` enumerates raw `Media[]`), so a Part-less `Media[0]` leaves no
  index-0 row and a pick of `Media[1]` is remembered.
- **Tick rule:** `mark_plays` is the single rule; `versions_of` drops Part-less rows, so a recalled id that is Part-less
  is "not listed" for the tick and `Gone` for the play: they agree. Cases tested: listed, not listed, index-0 row,
  Part-less `Media[0]` with two playable others, no versions (`mark_plays_for` skips), re-mark. The three wiring helpers
  (`episode_versions`, `prepare_panel`, `MarkPlaysVersion`) do have host tests in `app/version_ticks_tests.rs`; only the
  two call sites are untested.
- **Hygiene:** six commits each with the trailer; no address/token/household string (test fixtures `10.0.0.9`, `tok`).

## Decision: `Gone` forgets unconditionally

Acceptable. The memory records what last played; an explicit pick of a vanished id plays the Default, so the stale
remembered pick is no longer what the profile watched. It is an edge of an edge (stale menu after a rescan).

## Non-blocking

1. **Part-less `Media[0]` + remembered `Media[1]`: the tick over-claims.** `request_play_movie` returns false on
   `m.part.is_empty()` (`PmsMovie.part` = `default_part()`), before `request_play_inner` can recall, so a bare Play is
   refused while `mark_plays` ticks `Media[1]`. By code reading only. `docs/known-issues.md` ("a bare Play with
   nothing remembered is refused as before") implies it works with a pick. S13: reword; S15: look once.
2. **`ItemMenuScreen::opening()` now prefers the table cursor for EVERY page**, and `Focusable::reconcile` falls back to
   it, where it fell back to the opening row (never destructive). Doc says "until the viewer moves it". Intended only for
   the versions page; S15: a hold menu whose focused row vanishes while open (watchlist face arrives).
3. S13 list: batch 3 adds plan citations in `decision_version_memory_tests.rs:228` and `app/version_ticks_tests.rs:1`
   (the `grep version-delta` gate); `versions_panel.rs` module doc line 12 ("the server's first") predates the tick.
4. `mark_plays_for(row.sid, ...)` uses the row's `sid`; play uses `item_sid()` (surface fallback for `UNSET`). Only host
   rows have `UNSET`.
5. Remembered id + item fetch unanswered: `cached_item` is dropped (`env.version.is_some()`), so the play proceeds with
   no track lists, where a default play would use the cached detail. Remembered items only; same as an explicit pick.
6. S12/S14/S15 need nothing changed from this batch; the memory legs need a seated boot (`--mock-plextv`), as the notes say.
