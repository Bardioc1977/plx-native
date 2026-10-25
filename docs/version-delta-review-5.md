# Version delta, review 5: the merge of origin/main (a11469e8, 8522ac25, 4b176dc8)

Verdict: APPROVAL (no blocking item). Reviewer: independent read of the committed tree.

## Evidence

1. Merge shape. `a11469e8` has two parents (`c69a904b`, `b835afb8` = origin/main); `git log origin/main..HEAD
   --merges` lists only it; no squash, no rewrite; no tags or extra branches. `git diff origin/main..HEAD` has no
   conflict markers; its 63 files are exactly the delta (version memory, U1-U8, mock reconciliation, docs/tests,
   `ci/list-caps.ini` +8, the five ru lines). No upstream file is reverted.
2. Interplay with #1023/#1026/#1021/#1028/#1017/#1000/#950.
   - `fetch_playing_item` still runs on the resolve worker through the `&'static Client` of `build_stream`; it is not
     a store job, so #1023's "client resolved by the runner" does not apply to it.
   - `MetadataCmd::MarkPlaysVersion` is a synchronous main-thread edit of the loaded `Detail`/episodes; it emits no
     job, so #1026's drop rule has nothing to gate.
   - The memory door (`route::prefs::remember`) refuses unless `session::is_seated_as(user)` and the mirror is valid
     for the tenure; its file write is `session::queue_update`, dropped when tenure, client id or account token
     moved. That is the door's own moved-rule, the same one track memory has. #1025 is about ViewState admission and
     does not apply here; #830 (re-sign-in changes the account stamp) costs a version pick exactly as it costs a
     track pick (lost, never misfiled).
   - `settle_version` (`decision_memory.rs`) keys on `current_profile_key()` at the landing and `memory_scope`
     (`client_for(sid).machine_id()`). Sign-out/unseat: refused by the seat gate. Same-profile token refresh: same
     key, correct profile. Another account: tenure/stamp guard. Profile switch: see remark 1.
3. ru keys. `git diff 8522ac25..origin/fix/ru-missing-keys -- locales/ru` is empty; the commit touches only
   `locales/ru/browse.json` (+3) and `widgets.json` (+2); none of the five strings has a placeholder (en neither).
   Isolated, so it can be dropped when #1029 merges (the identical lines then merge cleanly).
4. Gates, committed tree, `git status --short` empty before and after: `make check-cargo` 0; `make test-hostsim`
   1178 passed; `make test-hostsim C=plx_media` 1956 passed, 64 ignored; `make check-python-harness` Ran 544 OK
   (skipped=1); `make check-nodef` 0; `make lint` 0; `make check-replay` 3 fixtures, 6 replays, 0 failures;
   `ci/check-deps.sh` all green (workers 9, globals 66, forks 58, fxlanes 11); `check-module-layers` green;
   `check-list-caps` 289 sites. `git diff c69a904b..HEAD --stat -- ci/allow` is deletions only (upstream's shrink);
   our only `ci` change is `ci/list-caps.ini`.
5. Shipped tree. `git grep version-delta -- docs rust-modules ci tests README.md ':!docs/version-delta-*'` is empty.
   DCO: `CONTRIBUTING.md` asks for `Signed-off-by`; none of the 32 commits in `origin/main..HEAD` has it (they carry
   `Co-Authored-By`).

## Non-blocking remarks

1. `settle_version` takes the profile at the landing, not at the request. A wrong-profile write needs the landing
   applied after a profile switch. Through the UI that is unreachable: the Player page is on top during Resolving
   (`prepare_start` flips to it), no account menu there, and BACK runs `exit_player` -> `cancel_play`, which drops the
   landing by generation. The only theoretical path is a background auth event rooting the picker
   (`bridge.rs` phase `Profiles|Switching`, no `cancel_play`) with a resolve slower than the user's pick. Cheap
   hardening, worth doing before the upstream PR: store `current_profile_key()` on the request and make
   `settle_version` refuse when it differs, plus one test. Blast radius today: one tick/default for one item.
2. DCO before any PR to upstream: every commit that lands needs `Signed-off-by: Heiko Kopp <...>` (no history rewrite
   now). Per AGENTS.md, `main` takes one squash commit: put the trailer into the squash message (GitHub's squash
   uses the PR title/body; add it there, or `git commit -s` on a squash prepared in a throwaway branch and
   opened as the PR). Keep the `Co-Authored-By` line, add the sign-off of the human.
3. `docs/version-delta-*.md` are branch-only; the PR must exclude or drop them (the plan says so).
4. `ci/test_disk_preflight.py` still fails its free-space assertion on this machine (not ours).
