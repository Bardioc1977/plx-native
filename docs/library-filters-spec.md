# Library filters

Spec for filtering a library listing by the facets the server declares. Status: **proposed, nothing
built.** Third of the Plex-server parity specs (`docs/parity-specs.md`).

**Scope: the Library screen's main listing (Movies, TV Shows, Seasons, Episodes) on a user's own
PMS.** Not the Collections listing (type 18, no facets), Search, the Watchlist, hubs, or playlists.

## 1. The gap

The server tells the client exactly which filters a listing supports, and the client uses two of
them:

- `GET /library/sections/{key}/all?includeMeta=1` returns `Meta.Type[].Filter[]`, each
  `{filter, filterType, key, title, type}`. `docs/pms-api.md` §2b records this, and "the client
  hardcodes no menu contents". Yet the Filter menu has an **Unwatched** toggle and a **Genre** row
  (`screens/src/library/menu.rs`, `filter_section`), and `QueryEdit` is
  `{Sort, Unwatched, Genre, LibraryType}` (`data/src/stores/browse.rs`). `SecState::query_filters`
  (`data/src/browse/mod.rs`) can emit nothing else, and `Meta.Type[].Filter[]` is not parsed.
- Everything the viewer can already do (Sort, A–Z rail, paging, Seasons/Episodes views, per-profile
  memory of the view in `library_sorts` / `LibraryView`) keeps working with any query; only the
  query builder and the menu are narrow.

### Measured on the owner's library (PMS 1.43.5.11029, 2026-10-24, GET only, counts and names)

| listing | advertised filters | sorts |
|---|---|---|
| Movies (and "Other videos", same shape) | 28 | 9 |
| TV Shows (`type=2`) | 15 | 8 |
| Seasons (`type=3`) | 0 | 2 |
| Episodes (`type=4`) | 17 | 7 |

Movie facets, in the server's order, with `filterType`: `genre` string, `year` integer, `decade`
integer, `contentRating` string, `collection` string, `director`, `actor`, `writer`, `producer`,
`country`, `studio` (all string), `resolution` string, `hdr`, `dovi`, `hdr10plus`, `atmos`
(boolean), `unwatched`, `inProgress`, `unmatched` (boolean), `videoCodec`, `audioCodec`,
`subtitleCodec`, `audioLayout`, `audioLanguage`, `subtitleLanguage`, `editionTitle`, `label`,
`location` (string). Show facets are `genre, year, contentRating, studio, network, country,
collection, director, actor, writer, producer, unwatchedLeaves, unmatched, editionTitle, label`
(a show's unwatched facet is `unwatchedLeaves`, which `query_filters` already special-cases).

Value lists (`GET {Filter.key}`, e.g. `/library/sections/1/genre`), movie library:

- `genre` 23 values, `year` 48, `decade` 7, `contentRating` 7, `resolution` 6, `videoCodec` 5,
  `audioCodec` 7, `audioLayout` 6, `audioLanguage` 12, `subtitleLanguage` 54, `country` 36,
  `studio` 215, `director` 332, **`actor` 807**, `location` 1; `collection`, `label` and
  `editionTitle` have **0** values here.
- Value rows are `{key, title}` plus `fastKey` (a ready listing URL) on tag-like facets and `thumb`
  on people. `resolution`, `audioLanguage` and the codec lists carry no `fastKey`.
- **Boolean facets have no value list**: `/library/sections/1/hdr`, `/dovi`, `/atmos` do not answer
  JSON. The `filterType` is the only signal; a boolean is a toggle that needs no fetch.
- The lists are **not scoped by the active filters**: `/genre` returns 23 values with and without
  `resolution=4k`. The client cannot ask "genres that remain".
- Composition works: `resolution=4k&hdr=1` matches 221 movies; `unwatched=1` 99; `inProgress=1` 20;
  `year>>=2015` (URL-encoded `year%3E%3E=2015`) 283.

So the server offers 28 filters on Movies and the app uses 2. The two it uses are also the cheapest
to build; the rest of this spec is the generic version of them.

## 2. Principles

1. **The server's declared list is the menu.** The facet set, its order and its titles come from
   `Meta.Type[].Filter[]` for the CURRENT listing type. No facet name is a menu entry in code. Code
   knows only how to *present* a `filterType` and a handful of known names (§3.3), and anything it
   does not know is still offered, as a generic multi-select.
2. **One query builder.** Every term goes through `SecState::query_filters`' successor. No screen
   writes a query string. The letter rail's `firstCharacter` counts and the paged `/all` listing must
   receive the same terms, or A–Z jumps land on the wrong offsets; today that holds for
   `unwatched`/`genre` and has to hold for every facet.
3. **Filters belong to a listing type.** Movies, Shows, Seasons and Episodes advertise different
   lists (Seasons none). A term is stored with the type it was set on.
4. **Presentation is the app's, capability is the server's.** The menu uses the existing popover and
   `MenuForm` rows (`ui/popover.rs`, `ui/table.rs`), the shared header spec, and the TV's own
   keyboard seam for text (`docs/search.md` §3). No official-client modal sheets.
5. **Lean and bounded.** A value list can have 807 rows. It is fetched on demand, paged, and never
   held for facets the viewer has not opened.

## 3. Behaviour

### 3.1 Terms

```rust
/// One active filter: a facet the server declared, and the chosen values.
pub struct FilterTerm {
    pub facet: String,        // Filter.filter, e.g. "genre", "resolution", "hdr"
    pub values: Vec<String>,  // value keys; a boolean carries ["1"]
}
pub struct FilterSet { pub terms: Vec<FilterTerm> }   // at most one term per facet, MAX_FILTER_TERMS
```

- **Across facets: AND. Within a facet: OR.** On the wire `genre=7,12&resolution=4k&hdr=1`. This
  matches the Plex Media Query language (`docs/pms-api.md` §2b: OR via comma).
- A string/integer facet takes keys from its value list; a boolean takes `1`. Keys are sent
  URL-encoded, values exactly as the server gave them (`key`, never `title`).
- Bounds: `MAX_FILTER_TERMS` (new constant, `ci/list-caps.ini`) and a per-term value cap
  `MAX_TERM_VALUES`; both exist so a stored view cannot grow the 256 KiB preference document.

### 3.2 Persistence

Today's `LibraryView` stores `sort`, `desc`, the genre and the Unwatched flag per profile and
library (`plex/src/plex/session.rs`, key `library_sorts`, additive-fields-only). The new terms are
**one additive field** (`filters`) on that record, omitted while empty, soft-parsed per element,
account-bound exactly like the record around it; no key is renamed and no `Mutation` or
`PublicPayload` slot is added (`plex/src/plex/CLAUDE.md`, *contract for a new setting*, step 5). The
existing `genre` and `unwatched` fields keep being written for rollback and read as terms:
a stored `genre=7` becomes the term `genre: ["7"]`, a stored Unwatched becomes `unwatched`/
`unwatchedLeaves` by listing type. `LibraryViews::CAP` and the document-size guard
(`prefs::shed_memory`) already bound the list; the new field is counted by the same measure and the
guard test is extended.

A stored term whose facet the server no longer advertises, or whose value no longer exists, is
dropped when the menus arrive and the listing is re-queried; an empty-result stored view never traps
the viewer (§3.6).

### 3.3 The menu

The Library header keeps its **Sort & Filter** control; its popover keeps Show, Sort and the quick
toggles. Filtering moves into a **Filters** panel reached from a **Filters** row in that popover (and
directly from the header's *Filter · N* control when terms are active, N being the number of terms).

The panel is a page-owned panel on the shared `ModalStack` (the `TracksPanel` / `AltSources` pattern
in `screens/src/registry.rs`), one row per advertised facet:

- Row: facet title (server `title`, localized by the app's table for known facets, §3.4), the
  current selection as the value ("Drama, Comedy", "4K", "On"), a chevron for lists, a switch for
  booleans.
- A boolean row toggles in place (`hdr`, `dovi`, `hdr10plus`, `atmos`, `unwatched`, `inProgress`,
  `unmatched`).
- A list row opens a **value picker**: a multi-select checklist (rows with a check, OK toggles,
  BACK returns), "Clear" at the top when anything is chosen. Counts per value are not shown (the
  server does not scope them, §1).
- Grouping, by facet name, presentation only: **Basics** (genre, year, decade, contentRating,
  unwatched, inProgress), **People** (director, actor, writer, producer), **Quality** (resolution,
  hdr, dovi, hdr10plus, atmos, videoCodec, audioCodec, audioLayout, subtitleCodec), **Language**
  (audioLanguage, subtitleLanguage), **More** (everything else, including unknown facets). Group
  order and membership live in one table; the server order decides the order within a group.
- A facet whose value list turns out empty (here `collection`, `label`, `editionTitle`) shows the
  sentence "None in this library" in its picker; it is not hidden, because hiding would need every
  list fetched up front.

### 3.4 Strings

Facet titles come from the server in the request language. For the known names the app has its own
i18n entries in every shipped locale (as `hub_title.rs` does for hubs), because a server title can
arrive in another language (issue #12's lesson); an unknown facet falls back to the server's title.
Value titles are the server's (genre names, language names, studio names) and are never translated.

### 3.5 Large lists

`actor` (807), `director` (332), `studio` (215) and `subtitleLanguage` (54) are too long to scroll.

- Pickers page their rows (`X-Plex-Container-Start/-Size`, both always sent; UNVERIFIED that value
  lists honour paging, §6) and keep only the visible window plus lookahead.
- A people or studio picker has a **Search** row at the top that opens the TV's keyboard and filters
  the list by title (server-side with a `title` query if the value endpoints accept one — UNVERIFIED —
  else client-side over the fetched pages up to the cap).
- Phase 1 ships the paging and search for any list over a fixed row count; short lists are plain.

### 3.6 The result

- The header shows the filtered listing's `totalSize` (the count the viewer cares about, not a
  "221 of 386" pair) and the *Filter · N* control.
- **Empty result:** the grid is replaced by one sentence ("No titles match these filters") with a
  focusable **Clear filters** action. A restored view that matches nothing opens this state, never a
  blank grid.
- **Clear filters** resets terms only, not the sort or the listing type.
- Changing a term restarts the listing at the top, keeps the sort, and goes through the same
  single-flight page fetch and view-ledger stamp as a sort change (`stores::browse`,
  `viewledger`), so a slow earlier page can never land on the new listing.
- Switching listing type keeps each type's own terms; terms the target type does not advertise are
  inert, not deleted.
- Marking an item watched while `unwatched` or `inProgress` is active already re-queries
  (`membership_depends_on_viewing`); the same hook covers those two facets in the generic form.

## 4. Data and wire changes

- `plex`: parse `Meta.Type[].Filter[]` (`Facet { filter, filter_type, key, title }`) leniently; a
  `facet_values(path, page)` read for a facet's `key`. Keys of `Filter` go through the existing lenient
  string adapters. The listing request takes `&[FilterTerm]`.
- `data::browse`: `SecState` replaces `unwatched: bool` and `genre: Option<GenreEntry>` with a
  `FilterSet` plus the cached facet list per type; `QueryEdit` gains
  `Facet { facet, values }` and `ClearFilters` (the `Unwatched`/`Genre` variants become thin
  constructors of the same edit so existing call sites and tests keep their meaning while they
  migrate). `genres`, `genres_done` and `genre_resolved` generalize to a per-facet value cache with
  a byte/row bound.
- Value lists are fetched by the store as typed jobs and returned as addressed events (store jobs,
  `docs/stores-as-machines.md`); the screen starts no worker.
- `screens`: the panel, the picker, the header control and the empty state, built from `MenuForm`,
  `Row` and the shared header spec; a new row kind only if `ui/` lacks one, and then in `ui/`.
- Layers: wire types in `plex`, state in `data`, presentation in `screens`; no new upward name, no
  ledger line in `ci/allow/*`.

## 5. Phases

**Phase 0 — probes** (§6), recorded here with names redacted.

**Phase 1 — generic facets.** Parse the facet list, `FilterSet`, the query builder for every
`filterType`, the panel with boolean toggles and multi-select pickers (paged), persistence and
migration of the stored genre/Unwatched, the header count, the empty state, mock fixtures, tests.
Acceptance on the mock: choose two genres and 4K, the grid shows the intersection and the rail's
jumps still land on the right items, relaunch restores it, Clear returns the full list, a stored
facet the server dropped disappears without an empty trap.

**Phase 2 — richer terms and entry points.**
- Range facets: `year` and `decade` as "from / to" using `>>=` and `<<=` (`docs/pms-api.md` §2b).
- Exclusion (`!=`) per value, if probe P4 confirms it.
- "More like this" from the Detail page: the genre, studio, country, content rating and year shown
  in About become focusable and open the Library with that single term applied (the Library takes an
  initial `FilterSet` in its `AppArg`, as `Collection` takes its id).
- Facet-scoped value counts if a probe shows an endpoint that returns them.

## 6. UNVERIFIED facts to probe in phase 0

All are `GET`s and may run on the owner's real server (read-only; see `docs/playlists-spec.md` §7).

1. Do value endpoints honour `X-Plex-Container-Start/-Size`, and return `totalSize`?
2. Do they accept a `title=` (contains) query for a typed search? Does `actor`'s `fastKey` order by
   name or by popularity?
3. What does the `Filter.type` field ("filter") mean; do any facets arrive as `type: "filter"` with a
   `subType` or different `key` shape on other types (episodes' 17)?
4. Negation: `genre!=7` and `genre!=7,12` semantics, and `resolution!=4k`.
5. Does `firstCharacter` honour every facet, or only some (the rail must agree with the grid)? Does
   `/all` with a `type=` plus a facet the type does not advertise return an error, an unfiltered
   list, or an empty one?
6. Is the facet list the same for a restricted profile? (Parental labels may hide facets; read-only
   check with a managed profile's token needs the owner's OK to use that token.)
7. Is the value list for `year`/`decade` ordered newest-first, and what are the keys (decade as
   `2010`)? Which listing keys does `contentRating` use (`de/16` style prefixes)?
8. Episodes: do `show.*` prefixed facets exist (`show.genre`), and does the `show.id` scoping rule
   in `docs/pms-api.md` still apply with other facets?

## 7. Verification

Host (`make check`; `make test-crate C=plx_plex|plx_data|plx_screens`; `make test-hostsim`): facet
parse (movie, show, episode, the zero-filter season, string-encoded `key`), query builder for each
`filterType` including URL-encoding and comma OR, the migration of a stored genre/Unwatched view
(byte-for-byte for a sort-only entry — the existing `LibraryView` serialization test), a stored
facet the server dropped, the 256 KiB guard with `filters`, empty value list, paging a 807-row
list, single-flight and ledger stamping on rapid edits, the rail receiving the same terms, empty-state
and Clear, focus travel through `FocusEngine`, text-fit per locale for the panel rows.

Simulator (`ui-sim`): the panel and pickers on `tests/mock_pms.py`. The mock today serves a `genre`
value list and filters on `unwatched` (`tests/mock_pms.py`; whether its `Meta` block carries a
`Filter[]` list is to be checked when the fixtures are written); it grows the facet list per type (movie 28, show 15,
episode 17, season 0), value endpoints for the common facets with a long list (so paging is
exercised), and the composition of terms. Replay fixtures stay synthetic: the mock reproduces the
measured shapes (boolean facets without value lists, unscoped value lists, zero-value facets)
without copying a household response.

Device (`tv-lock` first; `which-tier`): focus and scroll feel of the panel and a long picker on the
set, against the mock. Filters are reads, so a read-only pass against the owner's real server is
allowed for wire checks; it still does not play anything.

## 8. Out of scope

Saved filters/views; AND-grouping across values of one facet and nested boolean expressions
(`push`/`pop`/`or`); filters on Collections, Search, hubs or the Watchlist; creating smart
collections or smart playlists from a filter; server-side "Categories" browse pages (a filtered
Library covers them); editing metadata behind a facet (labels, editions).

## 9. Prose this change makes false (for `doc-claim-auditor`)

`docs/pms-api.md` §2b ("the client hardcodes no menu contents" becomes true for filters too, and the
`Meta.Type[].Filter[]` "not parsed" notes), `docs/parity-gaps.md` (theme C: "1 facet out of the
server's ~27"), `screens/src/library/menu.rs` and `data/src/browse/mod.rs` doc comments that say
"the genre and the Unwatched toggle", and any release-note or agent-reference line that lists the
Filter menu's contents.
