# Version delta: reviewer, batch 1 (S0-S5, `93d3a808..46bd9cc5`)

VERDICT: APPROVAL (no blocking issue).

## Own evidence

1. **U1 red/green.** Mutation at HEAD: `build_stream`'s `let src_kbps = played...` replaced by
   `env.src_kbps` (what the parent computes from `request_env`'s zero). `make test-crate C=plx_media
   T=version_tests`: 4 failed (the three U1 tests with `left: 0 right: 6000 / 7500 / 9000`, and
   `picking_the_default_row_plays_like_a_default_play_under_a_rung`: `start.mkv` vs the direct part),
   10 passed. Restored with `git checkout`. Call sites: all five `env.src_kbps` readers in `plan.rs`
   (transport fallback, `tentative_quality`, the adaptive `quality_policy`, `plan.src_measure`, the log
   line) read the one local; `cur_src` (`apply_plan`) carries `src_measure`, so a later quality change
   re-judges the picked file. Version `None`: `src_kbps == env.src_kbps`, identical. `played == None`
   (pick gone): the default's figure with the default's part, consistent. Trailer/extra: version is None.
   `media_video_kbps` (first video stream of the first Part of the picked Media) equals
   `convert_streams`' kept stream (`video.is_none()` guard) and `Detail.bitrate` is `Media[0].bitrate`:
   the same rule on both sides.
2. **U2 red/green.** Mutation: `side_reader_target` back to `ps.request.part`. Result:
   `side_reader_target_names_the_played_part_of_a_picked_version` FAILED (`left 960001`, `right 960777`),
   29 others passed. Restored. `cur_part` is installed in the one place `cur_part_id` is
   (`apply_plan`) and has the three construction sites; every `apply_plan` comes from `pump_play` with
   the request that spawned it, so for a default play `cur_part == request.part`. The subtitle PUT
   uses `cur_part_id` (unchanged, already the picked version's).
3. **S3 non-vacuity.** Mutation `env.version = None` in `request_env`: the two re-entry tests and
   `the_picked_version_rides_the_request_and_the_landing` fail. Mutation dropping `plan.src_acodec =`:
   the retranscode test and `a_picked_version_direct_plays_its_own_part_after_asking_about_it` fail.
   Restored; the tree was identical to HEAD (`git status --short` empty) after every run.
4. **Layer.** `plx_data::metadata::source_video_kbps(video, whole)` is a pure two-number rule in the
   data layer; `route::plan::source_kbps` is now a one-line wrapper with the same semantics
   (video if > 0, else whole), `PlayedVersion::video_kbps` uses it. `check-module-layers.py` green.
5. **Gates run (HEAD, clean tree):** `plx_data` 1210 passed; `plx_media` 1743 passed, 64 ignored;
   `plx_plex` 730 passed; `make test-hostsim C=plx_media` 1905 passed; `make lint`, `make check-nodef`,
   `ci/check-deps.sh`, `ci/check-module-layers.py`, `ci/check-list-caps.py` green;
   `python3 tests/test_harness.py` `Ran 543 tests ... OK (skipped=1)`.
6. **Ledgers.** `ci/` and `tests/` untouched by the five commits; the on-disk ledgers sum to
   16/78/58/11 and match `PINNED_LEDGER_TOTALS`. Goldens (S2) are additions only; no existing
   golden/fixture changed.
7. **Docs.** F1-F14 are counts and shapes only; the diff has no address, token, title or path (the
   only hits are `X-Plex-Token=tok` test fixtures). The known-issues section has the two required
   entries (hero chips stay on version 0; Part-less `Media[0]` has no way back to the Default) plus
   the one-playable-other loss. Tags: local `version-selection-archive` and `version-selection-final`,
   both at `be3b1c30` (= the tip of `version-selection`), harmless.
8. **Commits.** One commit per step (S0, S2, S3, S4, S5; S1 correctly empty), explicit paths, each
   with the Co-Authored-By trailer, S5 amended once.

## Decision: citations of `docs/version-delta-plan.md`

The plan settles (O4) that only the spec and the F-facts land on `main`; the plan stays on the branch.
Then these must not cite it at squash time: `docs/version-selection-spec.md` (status header, section 7,
section 10), `docs/known-issues.md`, `docs/pms-api.md`, the comments in
`plex/src/plex/transcoder.rs` and `route/decision_version_default_tests.rs`. Decision: acceptable on
the branch now (one rewrite at the end is cheaper than rewriting each step), but S13 must make
`grep -rn 'version-delta' docs rust-modules ci tests` empty outside the `docs/version-delta-*.md`
files, repointing to the spec (and the spec's section 10 list must be inlined, not "see plan section 7").
Add that grep to the S13 row of the plan so it is not forgotten.

## Non-blocking

1. `Plan.part` is set after the early exits of `build_stream` (no client, direct play disabled): those
   plans carry `part_id` from the argument but `part == ""`. They have no URL and never start a reader;
   setting `part: part.to_string()` in the initial struct literal would make the pair uniform.
2. Spec status header says "implemented upstream + delta" while only U1/U2 of the delta exist; it is the
   header the plan prescribes for the squash, so fine, but it is only true at S14.
3. U1 makes a picked Default row (index 0) measure from the fetched file, while a default play from a
   source where `detail_describes` is false (Up Next, season list) still gets 0 and fails closed on a
   fixed rung. The pick is the more accurate side; a "same as default" claim in the plan holds only
   when the page describes the item. No code change needed; worth one sentence in the PR text.
4. `make check` as a whole was not run here (its known `ci/test_disk_preflight.py` failure stops the
   cargo branch); the halves above were run instead.
