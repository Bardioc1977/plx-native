# Disguised libraries (hide a library from global views, keep it searchable on its own)

Spec for an opt-in "disguise" mode for a library, first of all the *Other videos* kind. Status:
**proposed, for later; nothing built.** It builds on the Other-videos work for upstream issue #599
(a separate library kind and strip pill) and on library-scoped search. Owner request, 2026-10-24.

## 1. What the owner wants

A library marked **disguised** is left out of everything "global": Home rows, global search results
and the global favourites/summary surfaces. When the viewer **opens that library explicitly** and
searches there, the search works, and it is **limited to that library**. Plex has a visibility
setting per library, but the official client does not give this combination (hidden from global
search yet searchable on its own), which is why the app needs its own switch.

## 2. What is known

Measured on the owner's PMS (1.43.5, GET only):

- A section carries a `hidden` integer on `/library/sections` (0 on the movie and show libraries,
  **1** on the Other-videos library). Its preference record (`GET /library/sections/{id}/prefs`)
  exposes it as the setting **`hidden`, label "Visibility", int, default 0**, with no value list.
  What 1 and any higher value mean is **UNVERIFIED**; Plex's own UI offers a visibility choice, and
  the observed effect is that the server's global `/hubs` and Continue Watching carry nothing from
  the hidden section while the library's own `/hubs/sections/{id}` still answers
  (measured in the #599 analysis: four section hubs present).
- The app reads none of it: `LibrarySection` has `key`, `type`, `title`, `size` only
  (`plex/src/plex/models.rs`).
- **Search cannot be scoped by the server.** `docs/pms-api.md` ("`sectionId` RANKS; it does not
  filter"): with `sectionId=N` every other section's hits are still returned, only the hub order
  changes. A library-scoped search therefore has to be a **client-side filter** on the rows'
  `librarySectionID` (present on rows from `/hubs/search`; to be confirmed per hub type).
- The search screen today is account-wide and merges all servers; favourites only RANK
  (`data/src/search.rs`, module doc: "a favourite is a browsing preference, not an authorization
  boundary"). That rule stays: **disguise is the viewer's explicit exclusion, a different thing from
  not-a-favourite**, and must be its own setting with its own wording.

## 3. Design

1. **A per-library flag, `disguised`, stored by the app per profile** (not by writing the server's
   `hidden`; the server is never modified by this feature). It is a new additive list in the
   preferences following the "contract for a new setting" (`plex/src/plex/CLAUDE.md`): Profile
   scope, account-bound, soft-parsed, capped, keyed by `(machine_id, section key)`.
2. **Where it is set:** the *Favorite libraries* editor and the Sources panel gain a row per library,
   **"Hide from global search and Home"**, offered for every library but meaningful first for Other
   videos. Default off. A server-side `hidden = 1` library may default to disguised on first sight
   (the owner already hides it there); this is a decision (§6).
3. **What a disguised library is left out of:** the global search merge, Home hub rows, Continue
   Watching, and any "all libraries" summary. It stays in the Library screen, the Sources panel and
   its own strip pill (from #599), always reachable on purpose.
4. **Search inside a library:** the Library screen gets a **Search** control (shared header spec).
   It opens the existing Search screen with a **scope** set to that library: the request is the
   usual `/hubs/search`, the result rows are filtered to `librarySectionID == scope`, the scope is
   shown as a chip with a way to clear it, and a disguised library is searchable only in this
   scoped mode. Without a scope, disguised libraries contribute no rows. The scope is part of the
   search request identity (generation and cache keys), so a stale unscoped answer can never land on
   a scoped screen.
5. **Hub-level rule:** the search merge drops rows by section before ranking, so counts, tags
   ("tag straddling") and empty-state wording are computed on what remains.

## 4. Edge cases

- All libraries disguised → global search says "nothing to search" with a hint to search inside a
  library, never an empty list without explanation.
- A disguised library on a shared server follows the same key `(machine_id, section key)`.
- A section vanishing from the server drops its flag at the next roster read.
- Sign-out clears the list with the other account-bound preferences.
- Scoped search to a library that is also a favourite: the favourite ranking is irrelevant (one
  library) and unchanged elsewhere.

## 5. Phases

0. Probes (read-only): the values and meaning of `hidden` (compare a section set to each value on a
   scratch server, since changing it writes), `librarySectionID` presence per hub type of
   `/hubs/search`, whether `/hubs/search` honours `includeCollections` for scoped results.
1. Preference + Sources/Favorites row + exclusion from Home and global search.
2. Scoped search from the Library screen with the scope chip.
3. Optional: default-disguised from server `hidden`.

## 6. Decisions for the owner

- Name of the setting (disguise vs hide) and its user-facing wording in all 9 locales.
- Whether a server-side `hidden = 1` library defaults to disguised.
- Whether Continue Watching from a disguised library is hidden too (proposed: yes).

## 7. Out of scope

Writing the server's `hidden` preference; parental-control style locking (PIN); disguising a
library's title or posters in lists; per-item hiding.

## 8. Verification

Host: the search merge with a disguised section dropped (counts, tags, empty state), scope
filtering by `librarySectionID`, request-identity/generation with and without scope, preference
round trip and account-bound clear. Mock: `tests/mock_pms.py` needs `hidden` and per-section
`librarySectionID` on search rows. Reads against the real server are allowed (GET only); nothing in
this feature writes to it.
