# Library filters

Spec for filtering a library listing by the facets the server declares. Status: **proposed; the data
half (wire, model, persistence, value-list jobs) lands first in two PRs that change no pixel, the
panel and the picker's search follow in two more (section 8).** Third of the Plex-server parity specs. Every number below was measured by
read-only GET on PMS 1.43.5 (counts and shapes only; the mock reproduces the shapes with synthetic
strings).

**Scope: the Library screen's main listing (Movies, TV Shows, Seasons, Episodes) on a user's own
PMS.** Not the Collections listing (type 18, no facets), Search, the Watchlist, hubs, or playlists.

## 1. The gap

The server tells the client exactly which filters a listing supports, and the client uses two of
them:

- `GET /library/sections/{key}/all?includeMeta=1` returns `Meta.Type[].Filter[]`, each
  `{filter, filterType, key, title, type}`. `docs/pms-api.md` §2b records this; "the client hardcodes
  no menu contents" is true for the Sort menu only. The Filter menu has an **Unwatched** toggle and a **Genre** row
  (`screens/src/library/menu.rs`, `filter_section`), `QueryEdit` is
  `{Sort, Unwatched, Genre, LibraryType}` (`data/src/stores/browse.rs`). `SecState::query_filters`
  (`data/src/browse/mod.rs`) can emit nothing else; `Meta.Type[].Filter[]` was not parsed until the wire PR (1a) of this spec.
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

The union of facet names the server advertises across the four listings is 30. Value lists
(`GET /library/sections/{key}/{facet}`, e.g. `/library/sections/1/genre`), movie library:

- `genre` 23 values, `year` 48, `decade` 7, `contentRating` 7, `resolution` 6, `videoCodec` 5,
  `audioCodec` 7, `audioLayout` 6, `audioLanguage` 12, `subtitleLanguage` 54, `country` 36,
  `studio` 215, `director` 332, **`actor` 807**, `location` 1; `collection`, `label` and
  `editionTitle` have **0** values here.
  Also measured: `writer` 717, `producer` 866, `subtitleCodec` 7.
- Value rows are `{key, title}` plus `fastKey` (a ready listing URL) on tag-like facets (also `year`,
  `decade`, `contentRating`, `location`, `country`, `studio`) and `thumb` on people (`actor`,
  `director`, `writer`, `producer`). `resolution`, the codec lists, `audioLayout` and both language
  lists carry no `fastKey`.
- **Boolean facets have no value list, and one of them must never be requested**: `hdr`, `dovi`,
  `atmos`, `inProgress` and `unmatched` answer 404 (HTML), but `/library/sections/1/unwatched` answers
  **200 with the whole unwatched listing** (99 movies, 248 KB unpaged). The `filterType` is the only
  signal; a boolean is a toggle that needs no fetch, and a reader that GETs a boolean facet's key
  downloads a listing.
- **A value request that carries filter terms is scoped by them**, and also by its own facet: `genre`
  returns 23 values bare, 13 with a `year` term, 21 with `unwatched=1`; `actor?unwatched=1` lists 259 of
  807; `genre?genre=<id>` lists only the genres that co-occur with that one. A value-list request without
  terms is the full, stable list. (The 23 genres with and without `resolution=4k` is a coincidence: every
  genre has a 4K title.) Phase 1 sends no terms to a value endpoint; scoping is Phase 2.
- **Value lists page.** `X-Plex-Container-Start` / `-Size` work as headers and as query parameters;
  a paged answer carries `size`, `totalSize` and `offset`, an unpaged one carries neither total nor
  offset; `Size=0` is a count-only read; a `Start` past the end answers no rows with `totalSize` still
  present. As on the listing, a `Size` without a `Start` is ignored.
- **No value search exists.** `actor?title=<s>` filters the ITEMS behind the values, not the tag names;
  `tag`, `q`, `search`, `prefix`, `contains`, `name`, `text`, `beginsWith`, `query`, `filter` and
  `firstCharacter` are ignored. (`/hubs/search?query=..&sectionId=..` returns `actor` and `director` hubs
  whose rows carry `filter: "<facet>=<key>"`.)
- **Order is not one rule**: `studio`, `genre`, `country` are case-insensitively alphabetical; `year`
  and `decade` newest first; `actor`, `director`, `writer`, `producer` follow neither title nor
  frequency order (opaque server order).
- **Keys arrive already percent-encoded** (`Not%20Rated`, audioLayout `5%2E1`, a studio key `X%2520Y`,
  that is a name encoded twice); the server decodes what it is sent twice (HTTP layer, then the tag
  matcher; `audioLayout` once only). A client must send each key verbatim. Re-encoding breaks the
  double-encoded ones (`X%252520Y` matches nothing; `audioLayout=5%252E1` matches nothing), while a
  single-encoded key survives one more encoding by luck (`Not%2520Rated` 23, `Not%252520Rated` 0). 187 of
  215 studio keys carry `%25xx`: verbatim matches 1, re-encoded 0.
- Composition works: `resolution=4k&hdr=1` matches 221 movies; `unwatched=1` 99; `inProgress=1` 20;
  `year>>=2015` (URL-encoded `year%3E%3E=2015`) 283. Across facets the terms AND; a comma inside one
  term is OR (`genre=<a>,<b>` is the union; `%2C` is accepted as the comma); a REPEATED parameter is AND
  (`genre=<a>&genre=<b>` is the intersection). Negation `genre!=<a>` is total minus `genre=<a>`.
  Operators are declared per field type in `Meta.FieldType[]` (`integer`: `=`, `!=`, `>>=`, `<<=`;
  `tag` and `boolean`: `=`, `!=`; `resolution`: `=` only, and `resolution!=4k` is silently treated as
  `=`).
- **Advertised-only is a menu, not a whitelist.** An unknown facet name is ignored (`foo=1` returns
  everything), but an unadvertised one is unpredictable: `unwatchedLeaves=1` on movies answers 0, `hdr=1`
  on shows is evaluated through episodes, `type=4&genre=<id>` answers 0 while `type=4&show.genre=<id>`
  answers 218. A bogus value (`genre=99999999`) answers 0, not an error.
- **Scope of a value list.** The `key` of a `Filter[]` record carries `?type=2` (shows) or `?type=4`
  (episodes) on most facets, but not all (shows' `country`, `unmatched`, `editionTitle`; episodes' codec,
  layout and language facets, `unmatched`, `location`). The scope changes the answer exactly where the
  server's key carries it: `year` lists 51 values untyped, 20 with `type=2` on shows, 50 with `type=4`;
  shows' `actor` 121 / 99; shows' `director` 264 untyped and 0 with `type=2` (directors hang off
  episodes). Everywhere else the list is the same with and without the listing's type.
- `Filter.type` is always the literal `"filter"`; `filterType` is `boolean`, `string` or `integer`.
  `thumb` is present on `actor`, `director`, `writer`, `producer`.

So the server offers 28 filters on Movies and the app uses 2. The two it uses are also the cheapest
to build; the rest of this spec is the generic version of them.

## 2. Principles

1. **The server's declared list is the menu.** The facet set, its order and its titles come from
   `Meta.Type[].Filter[]` for the CURRENT listing type. No facet name is a menu entry in code. Code
   knows only how to *present* a `filterType` and a handful of known names (§3.3), and anything it
   does not know is still offered, as a generic multi-select.
2. **One query builder.** Every term goes through `SecState::query_filters`' successor. No screen
   writes a query string. The letter rail's `firstCharacter` counts and the paged `/all` listing must
   receive the same terms, or A–Z jumps land on the wrong offsets. Since upstream issue #869 (fixed by
   PR #1030) that holds for Unwatched and the genre; it must keep holding for every facet.
   `firstCharacter` honours every facet tested (24 combinations on movies, 7 on shows and episodes) and
   its counts sum to the filtered `totalSize` each time.
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
- A string/integer facet takes keys from its value list; a boolean takes `1`. Keys go on the wire
  VERBATIM (`key`, never `title`): they are already percent-encoded by the server, so a client never
  encodes them again; it only escapes bytes that are not already part of a `%XX` triple. One exception:
  a single-encoded `%2C` inside a key is rewritten to `%252C`, because the server reads `%2C` as the
  comma after the first decode. (That rewrite is exercised against the mock only; the one key seen
  with a comma on the real server did not discriminate.)
- Each value is the shape `{id, title}`; terms compare by id only, so a refreshed title is not a
  different query.
- Bounds: the live selection is capped by structure and by the measured URL limit only (the server
  answers 200 up to at least 8 KB of filter values and 500 at 16 KB; the app keeps its filter bytes
  under 4 KiB). The REMEMBERED copy is bounded by bytes (1.5 KiB per view) and drops whole terms from
  the end, never cutting the values of one, so a remembered term means what it meant.
- The Unwatched switch is not a term: it stays one shared flag of the section (it carries across
  listing types), mapped to `unwatched` or `unwatchedLeaves` by listing, and needs no `Meta`.

### 3.2 Persistence

Today's `LibraryView` stores `sort`, `desc`, the genre and the Unwatched flag per profile and
library (`plex/src/plex/session.rs`, key `library_sorts`, additive-fields-only). The new terms are
**one additive field** (`filters`) on that record, omitted while empty, soft-parsed per element,
account-bound exactly like the record around it; no key is renamed and no `Mutation` or
`PublicPayload` slot is added (`plex/src/plex/CLAUDE.md`, *contract for a new setting*, step 5). The
existing `unwatched` and `genre` fields stay authoritative where they already were: `unwatched` is the one
store of the Unwatched switch, `genre` is the own listing's genre until another term exists, and a
stored `genre=7` is read as the term `genre: ["7"]`. Terms are stored per listing type. An older build
ignores `filters`, edits the legacy fields and keeps `filters` verbatim when it rewrites the record.
`LibraryViews::CAP` and the document-size guard (`prefs::shed_memory`) already bound the list; the new
field is counted by the same measure and the guard test is extended.

A stored term whose facet the server does not advertise is dropped when the menus arrive and the
listing is re-queried, except that an empty facet list never proves a facet gone and the Unwatched
switch is never reconciled. Only the genre is checked against its value list at restore (one read, as
today); every other value is trusted, and a stale one matches nothing (the server answers 0, not an
error), so an empty-result stored view never traps the viewer (§3.6).

### 3.3 The menu

The Library header keeps its **Sort & Filter** control; its popover keeps Show, Sort and the quick
toggles. Filtering moves into a **Filters** panel reached from a **Filters** row in that popover (and
directly from the header's *Filter · N* control when terms are active, N being the number of terms).

The panel is a new kind of the Library's own anchored popover (`LibraryMenuKind`, `screens/src/registry.rs`,
whose Genre list is already an in-menu switch), not a new screen or `AppArg`, one row per advertised
facet:

- Row: facet label (the app's table for known names, the server `title` as a labelled fallback, §3.4), the
  current selection as the value ("Drama, Comedy", "4K", "On"), a chevron for lists, a switch for
  booleans.
- A boolean row toggles in place (`hdr`, `dovi`, `hdr10plus`, `atmos`, `unwatched`, `inProgress`,
  `unmatched`).
- A list row opens a **value picker**: a multi-select checklist (rows with a check, OK toggles,
  BACK returns), "Clear" at the top when anything is chosen. Counts per value are not shown (the
  server does not scope them, §1).
- Grouping, by facet name, presentation only: **General** (genre, year, decade, contentRating,
  unwatched, unwatchedLeaves, inProgress), **People** (director, actor, writer, producer), **Quality**
  (resolution, hdr, dovi, hdr10plus, atmos, videoCodec, audioCodec, audioLayout, subtitleCodec),
  **Languages** (audioLanguage, subtitleLanguage), **More** (everything else, including unknown facets:
  collection, country, studio, network, unmatched, editionTitle, label, location). Group
  order and membership live in one table; the server order decides the order within a group.
- A facet whose value list turns out empty (here `collection`, `label`, `editionTitle`) shows the
  sentence "None in this library" in its picker; it is not hidden, because hiding would need every
  list fetched up front.

### 3.4 Strings

PMS localizes `Filter[].title` per `X-Plex-Language`, but not usably for all nine shipped locales: all 28
`be` facet titles (and all 9 `be` sort titles) are the `ru` strings (issue #12's lesson, `hub_title.rs`),
and other locales carry wrong or untranslated words (hu "Légkör" for Atmos, pt "Subtitle Codec" in
English, de "Autor" for Writer, ru and be "Rating MPAA" for the age rating) and trailing blanks. So the
app has its own i18n table keyed by facet NAME in every shipped locale (the `sortfacts.rs` shape); the
server's title (trimmed; the facet name when empty) is the labelled fallback for a name the table does
not know. Value titles are the server's (genre names, language names, studio names) and are never
translated; some are localized by the server (audio languages), so a stored value title is refreshed
when a list lands.

### 3.5 Large lists

`actor` (807), `director` (332), `studio` (215) and `subtitleLanguage` (54) are too long to scroll.

- Pickers page their rows (`X-Plex-Container-Start/-Size`, both always sent; value lists honour paging,
  §1) and append pages in server order as the viewer scrolls.
- The server has no value search (§1), so a **Search** row in a people or studio picker, opening the
  TV's keyboard, filters client-side over the fetched pages. It is a separate, later PR because it is
  the only change that raises the keyboard.
- Until it ships, a long list is scroll-only.

### 3.6 The result

- The header keeps its "312 of 1,284 films" read-out while any term is on (`filter_active` generalizes
  from "Unwatched or a genre" to "any term") and gains the *Filters · N* control.
- **Empty result:** the grid is replaced by one sentence ("No titles match these filters") with a
  focusable **Clear filters** action. A restored view that matches nothing opens this state, never a
  blank grid.
- **Clear filters** resets terms only, not the sort or the listing type.
- Changing a term restarts the listing at the top, keeps the sort, and goes through the same
  single-flight page fetch and view-ledger stamp as a sort change (`stores::browse`,
  `viewledger`), so a slow earlier page can never land on the new listing.
- Switching listing type keeps each type's own terms; terms the target type does not advertise are
  inert, not deleted.
- Marking an item watched while Unwatched is on already re-queries (`membership_depends_on_viewing`
  tests the Unwatched flag and the `lastViewedAt` sort); the generic form adds `inProgress`.

## 4. Data and wire changes

- `plex`: parse `Meta.Type[].Filter[]` leniently into `Facet { name, kind, title }` (the record's `key`
  is deliberately not read); `Client::facet_values(section, facet, kind, type_terms, start, size)` reads
  a facet's value list from the facet NAME and the listing's type terms. Filter values go on the wire
  verbatim (`QueryBuilder::wire`).
- `data::browse`: `SecState` keeps the genre as the `genre` term of a `FilterSet` (the Unwatched switch
  stays its own flag), the facet list per type and a cache of value lists (three kept, pages appended in
  server order). The absolute edit `LibraryWork::Filters { set }` replaces the selection (the Library
  page's pending slot is last-wins, so a delta would lose the first of two toggles in one flush);
  `QueryEdit::Filters(FilterSet)` joins it with the panel, because `QueryEdit` is matched exhaustively
  by the Library screen. `QueryEdit::Unwatched` and `Genre` keep their meaning. The own listing's genre
  is stored in the record's legacy `genre` field only.
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

## 6. Probes, and their answers (read-only GET, PMS 1.43.5.11029, 2026-10-25)

1. Do value endpoints honour `X-Plex-Container-Start/-Size`, and return `totalSize`? Answer: yes, see §1.
2. Do they accept a `title=` (contains) query for a typed search? Does `actor`'s `fastKey` order by
   name or by popularity? Answer: `title=` filters the items, not the tags; the order is opaque (§1).
3. What does the `Filter.type` field ("filter") mean; do any facets arrive as `type: "filter"` with a
   `subType` or different `key` shape on other types (episodes' 17)? Answer: always the literal "filter", no `subType`; key shapes in §1.
4. Negation: `genre!=7` and `genre!=7,12` semantics, and `resolution!=4k`. Answer: §1 (`genre!=a,b` is none of the two; `resolution!=4k` is ignored).
5. Does `firstCharacter` honour every facet, or only some (the rail must agree with the grid)? Does
   `/all` with a `type=` plus a facet the type does not advertise return an error, an unfiltered
   list, or an empty one? Answer: it honours every facet tested; an unadvertised facet is unpredictable (§1).
6. Is the facet list the same for a restricted profile? (Parental labels may hide facets; read-only
   check with a managed profile's token needs the owner's OK to use that token.) NOT RUN: it needs a managed profile's token and the owner's OK. Nothing in the design depends on it.
7. Is the value list for `year`/`decade` ordered newest-first, and what are the keys (decade as
   `2010`)? Which listing keys does `contentRating` use (`de/16` style prefixes)? Answer: newest first; plain years, decade keys such as `2020` (title `2020s`); the rating strings as stored, no prefixes on the measured library.
8. Episodes: do `show.*` prefixed facets exist (`show.genre`), and does the `show.id` scoping rule
   in `docs/pms-api.md` still apply with other facets? Answer: `show.*` exists only as `Field[]` entries, never advertised as filters; `show.genre` and `show.id` work on `type=4` and compose with other facets.

## 7. Verification

Host (`make check`; `make test-crate C=plx_plex|plx_data|plx_screens`; `make test-hostsim`): facet
parse (movie, show, episode, the zero-filter season, string-encoded `key`), query builder for each
`filterType` including URL-encoding and comma OR, the migration of a stored genre/Unwatched view
(byte-for-byte for a sort-only entry — the existing `LibraryView` serialization test), a stored
facet the server dropped, the 256 KiB guard with `filters`, empty value list, paging a 807-row
list, single-flight and ledger stamping on rapid edits, the rail receiving the same terms, empty-state
and Clear, focus travel through `FocusEngine`, text-fit per locale for the panel rows.

Simulator (`ui-sim`): the panel and pickers on `tests/mock_pms.py`. The mock serves the measured facet
lists per type (movie 28, show 15, episode 17, season 0) in the server's `Meta` block, value endpoints
for them with a long list (so paging is exercised) and the composition of terms (comma OR, AND across
facets and repeated parameters, double decoding of keys, separate `unwatched` and `unwatchedLeaves`
predicates). Replay fixtures stay synthetic: the mock reproduces the measured shapes (boolean facets
without value lists, unscoped value lists when asked bare, zero-value facets) without copying a
household response.

Device (`tv-lock` first; `which-tier`): focus and scroll feel of the panel and a long picker on the
set, against the mock. Filters are reads, so a read-only pass against the owner's real server is
allowed for wire checks; it still does not play anything.

## 8. Cut line

Phase 1 is four PRs: **1a** the wire (facet parse, verbatim terms, value reads), the stored `filters`
field and the mock; **1b** the data layer (`FilterSet`, the genre as a term, value-list store jobs,
restore); **2** the Filters panel (the only PR that moves a screen shape); **3** the picker's Search row.
1a and 1b change no pixel and no string.

Not in Phase 1: year and decade ranges, negation, "all of" for a multi-select, "more like this" from
Detail, scoped value lists and counts, `show.*` facets on Episodes, people search through
`/hubs/search`, saved filters, the restricted-profile facet check, any filter on Collections, Search,
hubs, the Watchlist or playlists, new telemetry.

## 9. Out of scope

Saved filters/views; AND-grouping across values of one facet and nested boolean expressions
(`push`/`pop`/`or`); filters on Collections, Search, hubs or the Watchlist; creating smart
collections or smart playlists from a filter; server-side "Categories" browse pages (a filtered
Library covers them); editing metadata behind a facet (labels, editions).

## 10. Prose this change makes false (for `doc-claim-auditor`)

`docs/pms-api.md` §2b (the "27 filters" count and the "hardcodes no menu contents" sentence),
`docs/parity-gaps.md` (theme C: "1 facet out of the server's ~27"), `screens/src/library/menu.rs` and `data/src/browse/mod.rs` doc comments that say
"the genre and the Unwatched toggle", and any release-note or agent-reference line that lists the
Filter menu's contents.
