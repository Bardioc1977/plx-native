# Version delta: reviewer, batch 2 (S6-S8, `10ae30f0..d33f21c7`)

VERDICT: REJECT (one blocking item, small).

## Own evidence

- **Mutations (restored with `git checkout`, tree clean after each).**
  U3: `let scoped = false` -> `stream_rows_are_scoped_to_the_playing_part` FAILED.
  U6: drop `is_playable` filter in `versions_of` -> `an_unplayable_version_is_not_a_row_...` and
  `a_list_is_offered_for_two_playable_versions_...` FAILED; drop it in `playing_version_from` ->
  `a_listed_version_without_a_part_is_not_a_playable_pick` FAILED.
  U5: `resume_ns_for` bypassed -> `left: (Some(5700000000000), 5700000000000) right: (Some(0), 0)`.
  U4: `narrowed_decision` disabled -> `the_decision_doors_...`, `a_decision_about_another_version_leaves_the_door_empty`
  (plex) and `a_decision_about_another_version_is_never_judged_as_the_picks`,
  `a_refusal_in_a_body_about_another_version_does_not_refuse_the_pick` (media hostsim) FAILED.
- **Gates (clean tree):** plx_data 1219, plx_appkit 349, plx_screens 1708, plx_media 1747, plx_plex 735,
  hostsim plx_media 1909 passed; `make lint`, `make check-nodef`, `ci/check-deps.sh` (all green, ledgers
  untouched: `ci/` and `tests/` have no diff), `check-module-layers.py`, `check-list-caps.py` green;
  `tests/test_harness.py`: Ran 543, OK (skipped=1).
- No new i18n string (no locale file in the diff); no secrets/household strings in the diff; five commits each
  with the trailer; an amended S8a on a local branch that squashes anyway is fine.

## BLOCKING

1. **U5 clamp also hits live retries of a picked version.** `pump_play` (`media/src/route/decision.rs`,
   `resume_ns_for(resume_ns, played_dur_ms)`) clamps whatever `take_resume_for` returns, and
   `request_play_inner` fills the same `PLAY_RESUME` slot from `retry.resume_ns` (a quality/track re-resolve, the
   "contract changed in flight" re-entry, a rescue) as `arm_play_resume` does for the card/page point. A retry with
   the LIVE playhead at >= 95% of the picked version's own length (credits) therefore restarts the film at 0, while
   the same retry of a default play (`played_dur_ms == 0`) continues. `confirm_resume_presented` says the live
   playhead is the only truthful retry position. Evidence is the code path (a test cannot call
   `retry_current_play` after a successful landing: `begin_playback_request` refuses, the existing retry test uses a
   refused landing). Required change: clamp only a point that came from the card/page (`retry.is_none()` at request
   time; e.g. carry a flag next to the `(gen, ns)` pair, or clamp in the `arm_play_resume` leg), plus a test that a
   retry resume of 88 of 90 minutes on the pick stays 88.

## Non-blocking

1. **U7 is pinned for the fixtures, not for every codec id.** A throwaway fuzz (random 2-4 row lists over 12 video
   ids, 13 audio ids, 12 containers, sizes, every `SHIPPED` language; 1500 lists each) failed once or twice per
   language by 3-5 px on exotic wide ids, e.g. es `MSMPEG4V3 HDR · DTS-HD MA Mono · WEBM` 532 px in a 527 px column,
   fr `WMV3 · Dolby TrueHD Mono · M4V · 888,0 Go` 530 px. `DETAIL_UNITS = 120` made it clean but changes the pinned
   edge row (test needs a new expectation); `weight` could also give `W`/`M` 5 units.
2. U4 `Kept` for `listed <= 1` and by-position for unflagged bodies are mock-modelled only (labelled); a real
   PMS that lists a subset in a different order is unmeasured.
3. Offers/tick/pick: `offers_versions` is the only gate left (no `versions.len() >= 2` remains); every pick resolves
   by `Media.id`, never by list position; a Part-less pick plays the default exactly like a lost one. The cursor on
   the first row is harmless until S11 because nothing is ticked yet and every row plays.
4. `info_panel::audio_badge` still omits `dca-ma` (pre-existing, outside U8).
5. `docs/parity-gaps.md` still says "two or more versions" (S13's).
