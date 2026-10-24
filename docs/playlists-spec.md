# Video playlists

Spec for browsing and playing a PMS **video playlist**. Status: **proposed, nothing built.** Second
of the Plex-server parity specs (`docs/parity-specs.md`).

**Scope: video playlists (`playlistType=video`) of a user's own PMS, movies and episodes.** Audio and
photo playlists are out of scope, as are Plex Discover lists and the plex.tv Watchlist
(`docs/watchlist.md`), which is a different object on a different host.

## 1. The gap

The owner's server has five video playlists (about 190 entries); the client can do nothing with
them:

- Home's hub list carries `home.playlists` ("Recent Playlists", key `/playlists/all`). It is
  dropped on arrival: `data::pms::listable` admits only `movie | show | season | episode`, and
  `data/src/pms_catalog_commit_tests.rs` (`a_playlists_hub_reaches_no_shelf_so_its_key_is_not_admitted`)
  pins that. The title string `browse.home.hub.recent_playlists` and the localized-title table
  (`plex/src/plex/hub_title.rs`) already exist.
- `playlist` is not a playable kind (`data/src/metadata.rs`), and there is no `/playlists` endpoint
  in `plex/src`.
- `plex::Client::create_play_queue` builds a queue from one `ratingKey` only
  (`uri=server://…/library/metadata/{rk}`); there is no `playlistID=` variant.
- The queue window IS retained, as `plex::QueueRow`s (`plex/src/plex/timeline.rs`), but
  `route::with_queue` has no caller outside `media`, so no screen draws or jumps in a queue.

### Measured on the owner's library (PMS 1.43.5.11029, 2026-10-24, GET only, structure and counts)

- `GET /playlists` returns 5 entries, all `playlistType=video`, `smart=false`. Entry keys:
  `addedAt, composite, duration, guid, key, lastViewedAt, leafCount, playlistType, ratingKey, smart,
  summary, title, type, updatedAt, viewCount`. `composite` is a path
  (`/playlists/{id}/composite/{timestamp}`): the server renders the cover.
- `GET /playlists/{id}/items` returns plain `Metadata` rows (movies here, all from one library)
  with the extra field **`playlistItemID`**, present on every row. That id is the identity of the
  entry in the playlist; the same `ratingKey` can occur twice.
- **`leafCount` is not the number of returned items.** Four playlists: 61 vs 57, 38 vs 37, 8 vs 7,
  1 vs 1. The fifth says 84 and returns **0** items (HTTP 200, `size: 0`). PMS keeps entries whose
  media it can no longer resolve and counts them. A count shown to the viewer must come from the
  rows actually returned.
- **`duration` has two units.** In the `/playlists` list it is milliseconds (`233572000`); in the
  `/playlists/{id}/items` container it is seconds (`233572`). Sum the returned items' own
  `duration` instead of reading either.
- The container header of `/playlists/{id}` carries `addedAt, composite, duration, leafCount,
  playlistType, ratingKey, smart, type, updatedAt` and a `title`.

## 2. Principles

1. **A playlist is an ordered list, not a library.** Its order is the viewer's. No sort, no filter,
   no A–Z rail; a playlist page never reuses the library's sort machinery.
2. **Identity of an entry is `playlistItemID`**, never the `ratingKey` (a title can be listed twice).
   The queue's own identity is `playQueueItemID` (`QueueRow::item_id`).
3. **Count and duration are what was returned**, never `leafCount` or the container duration.
4. **Playlists are per server.** A playlist belongs to one PMS (`ServerId`); its entries are that
   server's `ratingKey`s. On a multi-source Home each server contributes its own playlists
   (`docs/shared-servers.md` §2), and a card carries its `ServerId` like every other card
   (`plex::same_item`).
5. **Shared mechanisms only** (AGENTS.md): a store that is a machine, a content page opened through
   `ContentArg`, the shared card/shelf/grid and `FocusEngine`, the item menu `Action` list, the
   existing play queue. No screen-local list widget and no hand-routed keys.

## 3. Data model

In `plx_plex::plex` (wire), `plx_data` (projection, store), `plx_screens` (page):

```rust
/// A playlist as the list and the Home hub give it.
pub struct Playlist {
    pub sid: ServerId,
    pub rating_key: String,        // /playlists/{id}
    pub title: String,
    pub summary: String,
    pub smart: bool,
    pub composite: String,         // cover path, token appended by the image path like any art
    pub updated_at: i64,
    pub last_viewed_at: i64,
}

/// One returned entry. A lean row: no Media/Part/Stream tree.
pub struct PlaylistEntry {
    pub playlist_item_id: i64,
    pub rating_key: String,        // the movie/episode on the same server
    pub kind: Kind,                // Movie | Episode
    pub title: String,             // episode: "Show · S2E3 · Title" is composed by the screen
    pub dur_ms: i64,
    pub resume_ms: i64,
    pub watched: bool,
    pub thumb: String,
    // plus the playback fields `QueueRow` already carries (part, vcodec, acodec) so OK can start it
}
```

- A page retains the `PlaylistEntry` list in that lean shape only, projected on the worker (a
  `Metadata` row carries the whole tree; the TV is 32-bit — `docs/parity-gaps.md` theme F). Entries
  are paged: the store follows `X-Plex-Container-Start` / `-Size` and the container's `totalSize`
  (or a short or empty page), exactly like the Watchlist list (`docs/watchlist.md`, *Wire facts*).
  Page size and a hard cap `MAX_PLAYLIST_ENTRIES` are new constants classified in `ci/list-caps.ini`.
- All numeric fields use the lenient `de_i64` adapters; one string-encoded number must not fail the
  whole container (`plex/src/plex/CLAUDE.md`, *Gotchas*).
- `playlistItemID` missing on a row (older server) is not an error: the entry keeps `0` and every
  lookup that needs identity falls back to its position. Phase 2's edits require the id and are
  disabled without it.
- Entries whose `type` is neither `movie` nor `episode` (a clip, a photo in a mixed playlist) are
  dropped at projection and not counted.

## 4. Behaviour

### 4.1 Reaching a playlist

No new strip pill. Home is where a viewer looks for "my playlists", and the server already offers
the hub:

- Re-admit the `home.playlists` hub as a shelf of **playlist cards**. `listable` gains a distinct
  playlist kind (not `Kind::Movie`), so a playlist can never be played, marked watched or put on
  the Watchlist as if it were a title. The shelf title is the existing localized string.
- The shelf pages through `/playlists/all` with the existing hub paging, so the full list is
  reachable without a separate screen (`is_pageable_hub_key` admits the key; the measured paging
  facts are in `docs/pms-api.md`, table row for `/playlists/all`).
- A playlist card: cover from `composite` (square art, UNVERIFIED size and aspect, §8), title, and
  the details line "N videos" (count of returned entries once known, else the hub's `leafCount`
  is NOT shown, a blank is better than a wrong number) per the Under posters policy
  (`registry::captions::label`).
- OK on the card opens the playlist page. Hold opens the item menu with a playlist's rows only
  (§4.4).

### 4.2 The playlist page

Opened through a new `ContentArg::Playlist` (as `ContentArg::Collection` is), owned by a new
`PlaylistStore` (`StoreId::Playlist`, appended last like `StoreId::Catalog`) and drawn by
`screens::playlist`.

- Header: title, summary when present, the count and total duration of the returned entries
  ("7 videos · 52 min"), and two controls: **Play** and **Shuffle**.
- Body: the entries in server order in the shared poster grid or list (the layout decision belongs
  to the UI restructure spec's grid rules; reuse the Collections grid cell, with the watched mark and
  a resume bar). Each tile's caption is the entry title; an episode adds its show line.
- OK on an entry plays **from that entry, with the playlist as the queue** (§4.3). Hold opens the
  ordinary item menu for the entry (Go to movie/episode/show, Mark watched, Play from start, …).
- Empty (a playlist whose entries all failed to resolve, like the measured 84-entry one): the page
  shows its sentence ("No playable videos in this playlist") and Play/Shuffle are absent. Never an
  endless spinner, never "84 videos".
- BACK returns to Home with the shelf position kept (the container's own restore).
- The page subscribes to the server's notification feed only through the store's existing
  `notify` path; it adds no connection (`plex/src/plex/notify.rs`).

### 4.3 Playing

`create_play_queue` gains a sibling for playlists:

`POST /playQueues?type=video&playlistID={id}&shuffle={0|1}&repeat=0&continuous=0&key=/library/metadata/{rk}`

(`key` only when starting from a chosen entry; parameter names are those of `docs/plex-openapi.json`.)

- **Play:** queue from the first entry. **OK on entry N:** the same queue with `key` = that entry.
  **Shuffle:** `shuffle=1`, server-side; the order the server returns is the order played.
- The queue response is projected exactly as today (`QueueRow` window, `next` successor), so the
  timeline report (`playQueueItemID`), resume, scrobble and Up Next need no new protocol.
- **End of a movie inside a playlist:** the next entry starts after the existing next-item
  countdown. Today `route::plan::up_next_of` and the credits-tail synthesis are episode-gated; a
  playlist makes them apply to a movie successor. The tile text becomes "Next in playlist" with the
  entry title. The Settings > Playback "next episode" switch governs this too: its label and
  `docs/playback-settings.md` entry change from "next episode" to "next item"
  (a naming change, not a new setting; key names are never renamed, `plex/src/plex/CLAUDE.md`).
- **Window exhaustion:** a playlist larger than the server's queue window (UNVERIFIED size, §8)
  needs the window re-centred on the playing item (`GET /playQueues/{id}?window=…&center=…`). The
  window refill belongs to the route/session machine as a typed effect and an addressed event, not
  a worker started by the screen (`docs/sans-io.md`).
- **Unplayable entry** (media gone, or refused by the server): playback skips to the next entry
  after one error read-out is shown for a short, fixed time; the playlist is never abandoned for
  one dead entry. If every remaining entry fails, the existing terminal error read-out appears.
- **Track and version memory** apply per entry unchanged (`track_memory`, and the version memory of
  `docs/version-selection-spec.md`).
- The queue's rows are never copied into the per-frame `publication()`: the `PlaybackSession`
  destructuring in `route/decision.rs` forces that decision for any new field, and the queue stays
  out of it. A queue list in the player (phase 2) reads it through a bounded accessor.

### 4.4 Item menu rows

- On a playlist card: **Play**, **Shuffle**, **Go to playlist** (when the hold is on a shelf).
- On an entry inside a playlist page: the ordinary rows. "Remove from playlist" arrives in phase 3.
- A playlist card never offers Mark watched or Watchlist rows (it is not a title).

### 4.5 Smart playlists

A smart playlist's `/items` is evaluated by the server on every request, so phase 1 treats it like
a plain one: read-only, order as returned, no editing rows. The owner has none, so this is
**UNVERIFIED** (§8) and is covered by a mock fixture and a tag on the card, not by a device test.

## 5. Layering

- `plex`: `Playlist`, `PlaylistEntry` wire types, `playlists()`, `playlist_items()` (paged),
  `create_playlist_queue()`. No UI names.
- `data`: projection, `PlaylistStore` as a machine with typed store jobs and addressed events (like
  `stores::collection`), `listable` split. `data` names `plx_plex` and below, never `net`/`media`.
- `media`: queue window refill and the next-item rule; nothing screen-shaped.
- `screens`: `playlist` page, card, menu rows, empty sentence; never names a sibling screen. The
  application routes `ContentArg::Playlist` (`app/input.rs`, same as collections).
- No `webos`/`port` name outside `[port webos]`. `ci/module-layers.ini` gets the new modules in
  their layers; `ci/allow/*` ledgers gain no line.

## 6. Phases

**Phase 0 — probes** (§8), results recorded here, redacted.

**Phase 1 — browse and play, read-only.** Hub re-admitted as playlist cards, `PlaylistStore`,
playlist page, Play/Shuffle/OK-on-entry, queue from a playlist, next-item continuation for movies,
empty and dead-entry handling, localized strings in every shipped locale, mock fixtures. No write
to the server beyond what playback already does (timeline, scrobble through the existing code).
Acceptance: on the mock, a 7-entry playlist plays through in order with a countdown between movies;
a shuffled start plays all entries once; a playlist of only dead entries shows the empty sentence;
the 84-entry shape never renders a count.

**Phase 2 — the queue as a surface.** A queue list in the player `…` menu (jump to a member,
current marker), Previous/Next item controls, repeat (`repeat=` semantics UNVERIFIED), "Play next /
Add to queue" using the same queue (`PUT /playQueues/{id}?uri=…&next=1`).

**Phase 3 — editing (writes).** Create (`POST /playlists?type=video&title=…&uri=…`), add
(`PUT /playlists/{id}/items?uri=server://{machine}/com.plexapp.plugins.library/library/metadata/{rk}`),
remove (`DELETE /playlists/{id}/items/{playlistItemID}`), reorder
(`PUT …/items/{playlistItemID}/move?after=`), rename, delete. UI: "Add to playlist…" in the item
menu (a popover listing playlists plus "New playlist…" which needs text entry — reuse the TV's own
keyboard seam, `docs/search.md` §3). Every write is explicit and confirmed in the UI; deleting a
playlist asks first. Phase 3 is gated behind the write rules in §7.

## 7. Verification and the real server

The owner has allowed testing against the real PMS **as long as nothing is written** to it. The
rules this spec follows:

- **Allowed against the real server:** `GET` only, from the host (`ssh` to the NAS with the token
  used on the NAS itself, never printed or stored), and from the app on the simulator in read-only
  mode. This covers `/playlists`, `/playlists/{id}/items`, `/hubs`, image fetches and wire-shape
  discovery.
- **Not allowed:** anything that changes server state. That includes `POST /playQueues`, because it
  creates a server-side object, and playback, because it reports timeline, resume and scrobble
  (`AGENTS.md`: never write to a real household's record). Queue behaviour, window refill, dead-entry
  skipping and every phase-3 write are therefore verified against `tests/mock_pms.py`, extended with
  playlist endpoints, not against the owner's server.
- **Replay fixtures stay synthetic.** `tests/mock_pms.py` documents why: committed fixtures use a
  closed alphabet so no household string reaches this public repository, and
  `tests/test_harness.py` verifies it. Shapes measured on the real server are re-created in the mock
  (same keys, same oddities: `leafCount` larger than the returned rows, an all-dead playlist,
  seconds-vs-milliseconds `duration`, missing `playlistItemID`), never copied from a response.
- A phase-3 write test may run against a **scratch** playlist on the owner's server only when the
  owner asks for it in that session; until then phase 3 is mock-only.

Host tests (`make check`; `make test-crate C=plx_plex|plx_data|plx_screens|plx_media`;
`make test-hostsim`): parse of both `duration` units, string-encoded numbers, missing
`playlistItemID`, a non-video row dropped, `totalSize` paging and a short last page, the dead-entry
count rule, `listable` still rejecting a playlist as a title, the hub re-admission test inverted
(the old test names the reverse behaviour and is rewritten, not deleted), queue request shape with
and without `key`, next-item rule for movies, Up Next text, store machine effects/events, focus
travel on the page through `FocusEngine`, text-fit per locale, `list-caps` classification.

Simulator (`ui-sim`): the shelf, the page, Play/Shuffle, an empty playlist. Device (`tv-lock` first;
`which-tier`): playback of a queue across two movies on the mock with `--guest`, the countdown
between entries, and a dead entry mid-queue. Replay and framedrop passes use the mock.

## 8. UNVERIFIED facts to probe in phase 0

1. Playlist cover: size and aspect of `composite` (fetch headers only, `HEAD`/ranged `GET`), and
   whether the image path needs the same `X-Plex-Token` handling as posters.
2. `/playlists/all` hub paging on this PMS: `totalSize` and the cap (the table in `docs/pms-api.md`
   measured 5 items).
3. Queue window size for `playlistID=` queues and the refill call. **Needs a queue, so it runs on
   the mock or with the owner's explicit OK** (creating a queue is a server write).
4. `repeat` and `shuffle` semantics on a playlist queue (OpenAPI says `repeat` wraps the window).
5. Smart playlist behaviour: items evaluated per request, ordering, `smart=true` row shape.
6. Whether `/playlists/{id}/items` omits entries the profile may not see (restricted profile), and
   whether it ever returns a non-video row in a `playlistType=video` list.
7. Whether a playlist that mixes servers' content can exist (expected: no, PMS playlists are
   per server).
8. Why PMS keeps dead entries and counts them in `leafCount`; whether the item rows carry a marker
   (such as an `Media`-less row) the client could use to label them rather than hide them.

## 9. Out of scope

Audio/photo playlists; Plex Discover lists; playlist sharing and import (`/playlists/upload`);
collaborative playlists; playing a library collection as a queue (a separate, smaller follow-up
reusing §4.3's `uri=` form).

## 10. Prose this change makes false (for `doc-claim-auditor`)

`docs/pms-api.md` (the `/playlists/all` row: "no row shows it"), `docs/parity-gaps.md` (playlists
under the inventory), `data/src/pms_catalog_commit_tests.rs` (the test that pins the drop),
`data::pms::listable`'s doc, `plex/src/plex/timeline.rs` (`create_play_queue` "for one item"),
`docs/playback-settings.md` and the i18n strings that say "next episode", and
`route/decision.rs`'s comment that `with_queue` has no caller.
