# Version delta: reviewer, batch 4 (S12-S15, `ebceeaf6..d08d396e`)

VERDICT: APPROVAL (no blocking issue).

## Own evidence

- **Gates (clean tree, HEAD):** plx_media 1779, plx_screens 1710, plx_data 1223, plx_plex 748, `make test-hostsim` (app) 1121,
  `make test-hostsim C=plx_media` 1941, all `test result: ok`; `make check-nodef`, `make lint`, `ci/check-deps.sh` ("all gates
  green"), `check-module-layers.py`, `check-list-caps.py` green; `make check-replay` 3 fixtures, 6 replays, 0 failures;
  `tests/test_mock_pms_library.py` Ran 103 OK (skipped=8, all catalog/ffmpeg-less), `mock_pms.py --selftest` ok,
  `tests/test_harness.py` Ran 544 OK (skipped=1). `git status --short` empty after every run.
- **23ca5414 red at the parent:** `git checkout 23ca5414^ -- decision.rs`: `a_pick_plays_from_a_card_whose_default_has_no_part`
  FAILED ("the pick is not refused"), `a_bare_play_on_a_partless_default_...` FAILED ("the remembered pick plays"). Restored.
- **Mock mutations (restored):** `requested_media_index` always 0 -> 6 `MultiVersion` failures; `all-unflagged` not stripping
  `selected` -> 1; Stream ids not unique per Part -> 1. The tests bite.
- Ledgers untouched (`ci/` diff on the branch is `list-caps.ini` +8 only), no `SHAPE`/`SCREEN_SHAPES_PIN` change, five commits
  each with the trailer, no address/token/household string. `grep -rn version-delta docs rust-modules ci tests README.md`
  is empty outside `docs/version-delta-*.md`; the repointed section numbers (spec 4.2 item 6, 4.3, 3, 6) exist.

## 1. The unplanned fix: real, minimal, correct

- **Defect real.** `request_play_movie` returned false on `m.part.is_empty()` before `request_play_inner` could recall or take the
  pick; `PmsMovie.part` is `Media[0]`'s, so a Part-less `Media[0]` made the Play Version row and the tick's "bare Play takes it"
  dead at the application, while the media tests (which call `request_play`) were green. This is review-3 remark 1, now resolved.
- **Entry paths.** Card play, restart, picked row (`input.rs` -> `captured_card` -> `request_movie`), hero Play with a catalog
  row (`PlayIntent::Movie`), item-menu Play: all go through this guard and are fixed. Hero Play without a catalog row
  (`PlayIntent::Item`), every episode play (`play_episode_value`), Up Next, trailers and previews call `request_play` /
  `request_preview` directly and never had this guard; they are unchanged. The guard reads the memory with the same
  `(item_sid, rk)` as `request_play_inner`, so the two reads agree. Unseated/guest: `recalled_version` is `None`, so the
  refusal is as before.
- **Edge (remembered id vanished, bare Play on a Part-less card): acceptable, keep the fix.** Measured with a throwaway host
  test (removed): stale id 9999, Part-less card, `request_play_movie(.., None)` starts, lands (`Some(0)`), plays
  `mediaIndex=0` and `stored == None` (the dead id is forgotten at the landing), so the next bare Play is refused as before.
  Reasons not to refuse: the card carries no version list, so the guard cannot know the id is stale without a fetch; and the
  episode and no-catalog-row paths already start the empty-Part Default with no guard, so the card refusal was the odd one.
  Unmeasured: what a real PMS answers for a Part-less `Media[0]` decision. A refused plan would NOT forget the stale id
  (settle needs a playable landing), so on a real server a bare Play could repeat the failure read-out until the viewer picks a
  row; the pick then overwrites the memory. No worse than the old silent no-op.

## 2. Mock

One flag with four modes, numeric unique `Media.id` (7000001+, first movie/episode renumbered), Part keys contain their Part
id (asserted), `mediaIndex` on decision and start (past the end: code 2000 / HTTP 400), 940005 Part-less `Media[0]` with two
playable others, stacked and nine-version fixtures, closed alphabet (tested), default-off equality (tested). Upstream's
`--multi-version` has no other user in `tests/run.py` or the harness scenarios (grep). README text matches the code
(`GET /_mock/requests` exists).

## 3-6. Docs, default path, S15 claims

Default path unchanged: the guard only widens for `part.is_empty()` cards. The S15 claims are plausible against the code, and
the not-device-verified list is honest and complete enough.

## Non-blocking

1. **README bullet states the feature flatly** while a picked version has never played on the TV and `mediaIndex` is mock-only
   (the README elsewhere says "hasn't been run on an affected set yet"). It is the plan's approved text; the owner may want a
   qualifier or to hold it until the device leg. "(a 4K and a 1080p copy, MKV or MP4)" reads like a requirement.
2. **known-issues "A bare Play with nothing remembered is refused (nothing happens)"** holds for the movie card and hero with a
   catalog row only. An episode, or a hero without a catalog row, starts the Default with an empty Part (code above). Say
   "a movie card".
3. **Spec 4.4 "remembered version vanished: play version 0, no error"** is, for a Part-less `Media[0]`, the empty-Part Default;
   one clause would say so (spec line ~198 only covers the remembered pick).
4. Code comments cite plan shorthand (`N4`, `(S15)` in `decision_version_memory_tests.rs`); the plan file stays, but `S15`
   has no home outside it.
5. The hold-menu cursor claim (review-3 remark 2) remains host-level, as the notes say.
