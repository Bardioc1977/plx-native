# User ratings and critic reviews

Spec for reading and writing the viewer's own star rating of a movie, show or episode, and for
showing the critic reviews a PMS carries. Status: **proposed, nothing built.** Last of the five
Plex-server parity specs (`docs/parity-specs.md`).

**Scope: ratings on a user's own PMS.** The three provider scores already on the detail page
(Rotten Tomatoes, IMDb, TMDB) are done and are not touched. Community ratings and reviews on plex.tv
(Discover), friends' ratings, and rating from the Watchlist/catalog page are out of scope (§9).

## 1. The gap

- **No `userRating` anywhere.** The only mention in the code is the sort-key table
  (`data/src/sortfacts.rs`: "an unlabelled rating still says which end is first"). `Metadata` does
  not parse `userRating` or `lastRatedAt`, `Detail` has no field for them, no card shows a mark, and
  there is no rate call in `plex/src` (the write calls are `scrobble`, `unscrobble`,
  `select_streams`, `remove_from_continue_watching`, and the Watchlist PUT/DELETE).
- **A viewer who rates in another Plex app sees the rating nowhere here**, and cannot set one.
- **No critic reviews.** The detail page shows scores; it never requests `includeReviews`, so the
  `Review[]` the server holds is never fetched or modelled.

### Measured on the owner's library (PMS 1.43.5.11029, 2026-10-24, GET only, counts and shapes)

- `userRating` is present on **7 of 386 movies**, on none of 33 shows and none of 1185 episodes:
  values `4.0` (2), `6.0` (1), `8.0` (1), `10.0` (3). It is a float on a **0–10 scale** (a star is
  2 points: 4.0 is two stars, 10.0 is five). An item that was rated carries `lastRatedAt` (epoch s).
  An unrated item has no field, not `0`.
- The query `sort=userRating:desc` works (the 10.0 items come first), but the movie listing's
  **declared** sort list (`titleSort, originallyAvailableAt, rating, audienceRating, duration,
  addedAt, lastViewedAt, mediaHeight, random`) does not contain it, so the Sort menu, which shows
  only declared sorts, never offers it. There is also **no `userRating` filter** among the 28 movie
  facets (`docs/library-filters-spec.md`).
- The flat provider scores are separate: `rating` (critic) and `audienceRating`, with
  `ratingImage` / `audienceRatingImage` naming the provider and icon state. `docs/pms-api.md`
  ("Review scores") already records that and the `Rating[]` array.
- **Critic reviews:** `GET /library/metadata/{rk}?includeReviews=1` returns `Review[]`; without the
  flag the array is absent. 38 of the first 40 movies carry reviews, 20 on the one inspected in
  detail. A review row has the keys `filter, id, image, link, source, tag, text`: `tag` is the
  critic's name, `source` the publication, `text` a **short excerpt (about 80 to 210 characters)**,
  `image` a publication logo and `link` the full review's URL. The excerpts are English.
- The write endpoint in `docs/plex-openapi.json` is `PUT /:/rate` with `key` (the `ratingKey`),
  `identifier` (`com.plexapp.plugins.library`), `rating`, and an optional `ratedAt`. Its body and
  the way to **clear** a rating are not documented there.

## 2. Principles

1. **The server's number is the truth; the star count is a view.** Store and send the 0–10 float.
   Show stars as `rating / 2`, allowing half stars (an odd value such as 7.0 is 3.5 stars). Never
   round the stored value to whole stars on read.
2. **A rating is view state, so it rides the view-state machinery.** The Watched toggle is the model:
   an optimistic edit published through the view ledger (`data/src/viewledger`, `viewreconcile`),
   reconciled on the next read, projected into every surface that shows the item, with the same
   rollback on failure. A rating must not become a second ad-hoc write path
   (AGENTS.md: never write an ad-hoc mechanism when a shared one exists).
3. **Writes are explicit.** A rating changes the household's record. It is only ever sent after an
   explicit viewer action, never inferred (no "rate on completion"), and it is never written by a
   test against a real server (§8).
4. **Per profile.** `userRating` belongs to the account that is signed in (the profile token); the
   app already sends the profile's token, so a managed profile rates into its own record.
5. **Shared UI only**: the popover + `MenuForm`/`TableView`, hero discs, the card mark components in
   `ui/`, the existing toast. No screen-local star widget; if `ui/` has no rating glyph row, the
   shared piece is built in `ui/`.
6. **Reviews are short quotes the server already licensed to the viewer's PMS.** The app shows the
   excerpt and its attribution, never fetches the linked article, and never opens `link` (a TV has
   no browser).

## 3. Data model

- `plex::Metadata` gains `user_rating: Option<f32>` (lenient: number or string, absent stays `None`,
  clamp to 0–10, a `0.0` read as *rated zero*, not unrated — the server omits the field when unrated)
  and `last_rated_at: i64`.
- `data::metadata::Detail` gains `user_rating: Option<f32>` and the `Review` list:

```rust
pub struct CriticReview {
    pub id: i64,
    pub critic: String,     // Review.tag
    pub source: String,     // Review.source (publication)
    pub excerpt: String,    // Review.text, bounded
    pub filter: String,     // the server's own filter key; not used for display
}
```

  Reviews are bounded (`MAX_REVIEWS`, `MAX_REVIEW_CHARS`, both classified in `ci/list-caps.ini`),
  dropped silently past the cap, and the logo `image` and `link` are not retained (not shown).
- Hub and listing rows (`PmsMovie` and the browse items) carry `user_rating` too, because the card
  mark and the hold menu read it. It is a single `f32`, no new tree.
- The `PmsMovie`/`Detail` view-state publication already has a per-item watched/progress record
  (`viewstate`); `user_rating` joins it so one write updates the hero, the card and the related
  strip together.

## 4. Behaviour

### 4.1 Reading

- The detail request adds `includeReviews=1` (one more parameter on the one metadata GET). Listing
  and hub requests need nothing: `userRating` is already in their rows (measured).
- A rated item shows the viewer's stars in the hero's rating row, first, before the provider
  scores, in the accent colour, so "mine" is visually distinct from "theirs". An unrated item shows
  nothing there.
- A card shows a small mark (the rating number or star glyph) only where the Under posters policy
  already shows rating (`registry::captions`); it never adds a row to a card that has none.

### 4.2 Rating an item

- **Control:** a **Rate** disc on the detail hero, beside the watched toggle (a new `HeroCtl`
  variant, present for a movie, show or episode that is on this server; never on a catalog-only
  page). OK opens a popover with five stars and a "Remove rating" row when one exists.
- **Item menu:** a **Rate…** row (new `Action::Rate(rk)`) on cards and episode rows, opening the
  same popover.
- **Input:** LEFT/RIGHT moves the highlight in half-star steps (0.5 to 5.0), OK commits, BACK
  cancels with no write. A number key press is not required. The current rating is the initial
  highlight; an unrated item starts **unselected** (no default rating is implied), and the first
  LEFT/RIGHT lands on 0.5 / 5.0 respectively.
- **Commit:** `PUT /:/rate?key={rk}&identifier=com.plexapp.plugins.library&rating={0..10}`, the
  rating sent as the 0–10 float (stars × 2). Optimistic: the hero and cards show it at once; on a
  failed write the previous value returns and a toast says so.
- **Clear:** "Remove rating" sends the server's documented clear value (UNVERIFIED, §7) and the
  field disappears. It is a deliberate row, not a long-press gesture.
- **Seasons and shows:** the server accepts a rating on a show or season; the control appears for
  them only if probe 3 shows the server stores and returns it. Otherwise phase 1 rates leaves (movie,
  episode) and shows.
- **Offline/relay:** the write goes through the same client and queue as scrobble; a failure is
  reported, never retried silently forever (one retry after the usual backoff, then the rollback
  toast).

### 4.3 Library integration

- The Sort menu shows only sorts the server declares, and this PMS does not declare `userRating`
  (§1), so there is no "My rating" sort today. `data/src/sortfacts.rs` already labels the key if a
  PMS ever declares it, and the generic filter menu (`docs/library-filters-spec.md`) would pick up
  a `userRating` facet the same way. The app does not add an undeclared sort or invent a client-side
  filter (it would need every row); whether to add `userRating` as an explicit extra sort, given
  that the query works, is a decision for the owner (§7, probe 9).
- When the current listing is ordered by `userRating` (declared by a future PMS), a rating change
  marks the listing membership stale through the same hook the Unwatched filter uses
  (`membership_depends_on_viewing`, extended with `userRating`).

### 4.4 Critic reviews

- A **Reviews** shelf on the detail page, below Cast & Crew, only when `Review[]` is non-empty:
  tiles with the excerpt (up to four lines), the critic name and the publication. OK on a tile opens
  the review in a sheet (the About panel pattern: `AppArg::AboutPanel`) with the full excerpt; there
  is no "open link".
- Shelf title and empty behaviour follow the Extras shelf. The shelf is skipped on a catalog-only
  page and for shows/episodes that carry no reviews.
- Source: PMS critic reviews, English text on this server. The app does not translate them and does
  not hide them for a non-English UI (the attribution makes the language obvious); a Settings
  switch is not added.

### 4.5 Edge cases

- Half stars from other clients (3.0 to 9.0 in odd values) display exactly; a value outside 0–10 is
  clamped on read.
- Re-rating the same value is a no-op (no request).
- A rating survives watching, unwatching and marking: they are independent fields.
- Rating an item on a shared server writes into the signed-in account's own record on that server
  (it is that server's view state); the card mark is keyed by `(ServerId, ratingKey)` like every
  other view-state fact.
- A restricted/managed profile whose server refuses the write (403) sees the failure toast and the
  prior value; the Rate control stays.

## 5. Layering

- `plex`: `rate()` beside `scrobble()` in `library.rs`; `user_rating` parse; the `Review` DTO.
  No UI names.
- `data`: projection into `Detail`/`PmsMovie`; the view-state ledger entry for a rating edit
  (a new `Scope`/edit kind next to watched), store job for the write, addressed result event.
- `screens`: `HeroCtl::Rate`, `Action::Rate`, the star popover, the reviews shelf and sheet; the
  application routes the actions (`app/input.rs`). Screens never name a sibling screen.
- `ui`: a star-row component (and its half-star glyph) only if absent; all colours and sizes from
  tokens.
- No `webos`/`port` name outside `[port webos]`; no allow-ledger line.

## 6. Phases

**Phase 0 — probes** (§7), recorded here.

**Phase 1 — read.** Parse `userRating`/`lastRatedAt`, the hero's "mine" stars, the card mark,
`includeReviews=1`, the Reviews shelf and sheet, locale strings in every shipped locale, mock
fixtures. No write. This phase is safe against the real server: it only reads.

**Phase 2 — write.** The Rate disc and menu row, the popover, the optimistic edit through the view
ledger, clear, rollback, the stale-listing hook.

**Phase 3 — shows/seasons** if probe 3 allows, and an optional rating shortcut in the player's
end-of-item card (explicit, never automatic).

## 7. UNVERIFIED facts to probe in phase 0

1. The method and body of `/:/rate` on 1.43.5: the OpenAPI says `PUT`, the older scrobble calls are
   `GET`; does the server accept both, and does it need `ratedAt`?
2. How to clear a rating (`rating=-1`, an empty value, or a separate call).
3. Whether a show or season rating is stored and returned (`userRating` on `type=2/3` rows).
4. Whether a half-star value (e.g. `7.0`) is stored exactly, or snapped to whole stars by some
   servers/clients.
5. Whether a managed (restricted) profile's token may rate.
6. Whether `lastRatedAt` is updated on a clear.
7. Whether PMS also syncs the rating to plex.tv's community/Discover rating (the official clients
   do for catalog items), and therefore whether a local rating becomes visible to friends — this is
   a privacy question for the Settings text, not a code question.
8. Review `text` language when the request carries another `X-Plex-Language`.
9. Whether `sort=userRating` is a stable, supported query on future servers (it is undeclared on
   1.43.5 though it works), which decides if an explicit extra sort is safe.

Probes 1 to 6 are **writes** (rate, change, clear). They are not run against the owner's server
(§8). Probe 8 and every read are GETs and may be.

## 8. Verification and the real server

The owner allows testing against the real PMS as long as nothing is written
(`docs/playlists-spec.md` §7). A rating is a write into a household record, so:

- Reads (phase 1): allowed against the real server — `userRating` rows, `Review[]`, `lastRatedAt`.
- Every write (phase 2 and probes 1 to 6): **only against `tests/mock_pms.py`**, which grows
  `PUT/GET /:/rate`, a settable `userRating`, `Review[]` for `includeReviews=1`, and a failure mode
  (403, timeout). If the exact wire form of probes 1 to 6 must be confirmed, it needs the owner's
  explicit OK in that session and a scratch item, and the rating is removed afterwards by the same
  call; until then the form follows the OpenAPI document.
- Replay fixtures stay synthetic (closed alphabet; review excerpts are drawn from the alphabet,
  never copied from a real review).

Host (`make check`; `make test-crate C=plx_plex|plx_data|plx_screens`): lenient parse of
`userRating` as number and string, absent vs `0.0`, clamp, `lastRatedAt`; `Review[]` parse with
missing keys and the caps; star ↔ value mapping including half stars and the round trip with no
drift; the rate request URL; the optimistic edit, reconcile and rollback in the view ledger;
publication to hero/card/related; the stale-listing hook under the `userRating` sort; Rate popover
focus and BACK-cancels-without-a-request; the reviews shelf visibility rules; text fit per locale.
Simulator (`ui-sim`): the mark, the popover, the shelf and sheet on the mock. Device (`tv-lock`
first): focus feel of the popover and legibility of the excerpts at couch distance, on the mock.

## 9. Out of scope

Plex Discover/community ratings and reviews, friends' ratings, rating from the catalog (Watchlist)
page, written reviews (the user's own review text), rating music or photos, ratings of collections
and playlists, automatic or implicit rating, and opening a review's `link`.

## 10. Prose this change makes false (for `doc-claim-auditor`)

`docs/parity-gaps.md` ("Rate & Review" under theme B and the ratings entries), `docs/pms-api.md`
("Review scores" section gains `userRating`, `lastRatedAt`, `Review[]`, `includeReviews`),
`data/src/sortfacts.rs` tests' wording about an unlabelled rating, and the Settings/Privacy text if
probe 7 shows a local rating reaches plex.tv.
