"""Library fixture contracts: the mock must exercise the real rail and sparse-page boundaries."""
import contextlib
import io
import json
import hashlib
import pathlib
import re
import shutil
import socket
import subprocess
import tempfile
import unittest
import urllib.parse
import urllib.request

from mock_pms import (
    ENHANCEMENT_AAC_51_RK, ENHANCEMENT_AC3_2CH_RK, ENHANCEMENT_DEFAULT_SRT_RK,
    ENHANCEMENT_DV_P8_RK, ENHANCEMENT_EXTERNAL_SRT_RK, EXTRA_MEDIA_RK_BASE, PLEX_DIRECT_HASH,
    CAPTION_EDGE_RK_BASE, RESUME_ON_DECK_SHOW_RK, RESUME_PLAIN_SHOW_RK,
    CatalogLibrary, Library, MockPms, demo_cache_dir, plaintext_only_lan_resources, serve,
)


HAS_FFMPEG_AND_FFPROBE = shutil.which("ffmpeg") and shutil.which("ffprobe")
HAS_DEMO_CACHE = bool(HAS_FFMPEG_AND_FFPROBE) and (demo_cache_dir() / "derived").is_dir()
CATALOG = pathlib.Path(__file__).resolve().parent / "demo_library" / "catalog.json"
FIXTURES = pathlib.Path(__file__).resolve().parent / "demo_library" / "fixtures"


def get(pms, path):
    status, content_type, data = pms.handle("GET", path)
    assert status == 200 and content_type == "application/json"
    return json.loads(data)["MediaContainer"]


class LibraryRail(unittest.TestCase):
    def test_catalog_recent_hub_pages_through_its_provider_key(self):
        lib = object.__new__(CatalogLibrary)
        lib.__dict__.update(Library(movies=60).__dict__)
        lib.catalog = {"hubs": [{"hubIdentifier": "home.movies.recent", "recent": "movie"}]}
        pms = MockPms(lib)
        response = get(pms, "/hubs/demo/home.movies.recent"
                           "?X-Plex-Container-Start=36&X-Plex-Container-Size=24")
        self.assertEqual(response["offset"], 36)
        self.assertEqual(response["Metadata"], lib.recent("movie", 1000)[36:60])
        self.assertEqual(response["totalSize"], len(lib.recent("movie", 1000)))

    def test_collection_ids_and_collection_routes_match_pms(self):
        pms = MockPms(Library())
        collections = get(pms, "/library/sections/1/collections")["Metadata"]
        self.assertTrue(collections)
        row = collections[0]
        self.assertNotEqual(row["index"], int(row["ratingKey"]))
        own = get(pms, f"/library/metadata/{row['ratingKey']}")["Metadata"]
        self.assertEqual(own, [row])
        children = get(pms, f"/library/collections/{row['ratingKey']}/children"
                            "?X-Plex-Container-Start=0&X-Plex-Container-Size=2")
        self.assertLessEqual(len(children["Metadata"]), 2)
        self.assertEqual(children["offset"], 0)
        via_all = get(pms, "/library/sections/1/all?type=18&sort=titleSort:asc"
                           "&X-Plex-Container-Start=0&X-Plex-Container-Size=2")
        self.assertEqual(via_all["Metadata"], sorted(collections, key=lambda c: c["titleSort"])[:2])
        status, ctype, body = pms.handle(
            "GET", f"/library/collections/{row['ratingKey']}/children", headers={
                "X-Plex-Container-Start": "0", "X-Plex-Container-Size": "1",
            })
        self.assertEqual((status, ctype), (200, "application/json"))
        self.assertLessEqual(len(json.loads(body)["MediaContainer"]["Metadata"]), 1)
        self.assertTrue(any(c.get("childCount") == 0 for c in collections),
                        "the fixture includes an empty collection")

    def test_collection_listing_matches_the_library_type_menu_contract(self):
        """The Library's Collections type: `all?type=18&includeMeta=1` declares only titleSort,
        `firstCharacter?type=18` counts collections, and only a collection with members has art."""
        pms = MockPms(Library())
        first = get(pms, "/library/sections/1/all?type=18&includeMeta=1"
                         "&X-Plex-Container-Start=0&X-Plex-Container-Size=60")
        kinds = first["Meta"]["Type"]
        self.assertEqual([k["type"] for k in kinds], ["collection"])
        self.assertEqual([s["key"] for s in kinds[0]["Sort"]], ["titleSort"])
        self.assertEqual(first["totalSize"], len(first["Metadata"]))
        letters = get(pms, "/library/sections/1/firstCharacter?type=18")["Directory"]
        self.assertEqual(sum(d["size"] for d in letters), first["totalSize"])
        self.assertEqual([d["title"] for d in letters],
                         sorted({row["titleSort"][0].upper() for row in first["Metadata"]}),
                         "the rail's letters come in the listing's titleSort order")
        for row in first["Metadata"]:
            self.assertEqual("thumb" in row, row["childCount"] > 0, row["title"])

    def test_collections_count_probe_reads_total_size_of_a_zero_size_type_18_window(self):
        """The header's "Collections · N": `all?type=18` with `Size=0` returns no rows and the
        section's collection count as `totalSize` (docs/pms-api.md section 2b)."""
        pms = MockPms(Library())
        listed = get(pms, "/library/sections/1/collections")["Metadata"]
        probe = get(pms, "/library/sections/1/all?type=18"
                         "&X-Plex-Container-Start=0&X-Plex-Container-Size=0")
        self.assertEqual(probe.get("Metadata", []), [])
        self.assertEqual(probe["totalSize"], len(listed))
        self.assertGreater(probe["totalSize"], 0)
        shows = get(pms, "/library/sections/2/all?type=18"
                         "&X-Plex-Container-Start=0&X-Plex-Container-Size=0")
        self.assertEqual(shows["totalSize"], 0, "a section with no collections counts zero")

    def test_a_smart_collection_is_opt_in_and_listed_with_smart_and_a_count(self):
        plain = get(MockPms(Library()), "/library/sections/1/collections")["Metadata"]
        self.assertTrue(all(str(r["smart"]) == "0" for r in plain), "the default data has none")
        lib = Library()
        lib.smart_collections = True
        pms = MockPms(lib)
        rows = get(pms, "/library/sections/1/collections")["Metadata"]
        smart = [r for r in rows if str(r["smart"]) == "1"]
        self.assertEqual(len(smart), 1)
        self.assertEqual(smart[0]["subtype"], "movie")
        self.assertGreater(smart[0]["childCount"], 0)
        self.assertEqual(len(rows), len(plain) + 1)
        probe = get(pms, "/library/sections/1/all?type=18"
                         "&X-Plex-Container-Start=0&X-Plex-Container-Size=0")
        self.assertEqual(probe["totalSize"], len(plain) + 1)

    def test_search_with_include_collections_answers_full_collection_rows(self):
        pms = MockPms(Library())
        listed = get(pms, "/library/sections/1/collections")["Metadata"]
        name = listed[0]["title"]
        query = urllib.parse.quote(name.split()[0])

        def hub(path):
            return next(h for h in get(pms, path)["Hub"] if h["hubIdentifier"] == "collection")

        # without the flag: tag rows under Directory — no ratingKey, no thumb
        tags = hub(f"/hubs/search?query={query}&limit=12")
        self.assertNotIn("Metadata", tags)
        self.assertTrue(all("ratingKey" not in t and "thumb" not in t for t in tags["Directory"]))
        # with it: the collection's own full rows under Metadata, both ids intact
        rows = hub(f"/hubs/search?query={query}&limit=12&includeCollections=1")
        self.assertNotIn("Directory", rows)
        self.assertEqual(rows["size"], len(rows["Metadata"]))
        row = next(r for r in rows["Metadata"] if r["title"] == name)
        self.assertEqual({k: v for k, v in row.items() if k != "score"}, listed[0])
        self.assertEqual(row["type"], "collection")
        self.assertNotEqual(row["index"], int(row["ratingKey"]))
        self.assertTrue(row["thumb"])
        # every other hub is unchanged by the flag
        plain = [h for h in get(pms, f"/hubs/search?query={query}&limit=12")["Hub"]
                 if h["hubIdentifier"] != "collection"]
        flagged = [h for h in get(pms, f"/hubs/search?query={query}&limit=12&includeCollections=1")["Hub"]
                   if h["hubIdentifier"] != "collection"]
        self.assertEqual(plain, flagged)

    def test_a_member_movies_related_carries_its_whole_collection(self):
        lib = Library()
        pms = MockPms(lib)
        member = next(it for it in lib.items.values()
                      if it["type"] == "movie" and it.get("Collection"))
        tag = member["Collection"][0]
        hubs = get(pms, f"/library/metadata/{member['ratingKey']}/related")["Hub"]
        own = [h for h in hubs if h["hubIdentifier"].startswith("collection.related.")]
        self.assertEqual(len(own), len(member["Collection"]))
        hub = own[0]
        self.assertEqual(hub["title"], tag["tag"])
        self.assertIn(f"tagId={tag['id']}", hub["key"])
        self.assertTrue(hub["key"].startswith(f"/library/sections/{member['librarySectionID']}/all?"))
        keys = [m["ratingKey"] for m in hub["Metadata"]]
        self.assertIn(member["ratingKey"], keys, "the member lists itself")
        self.assertEqual(keys, [m["ratingKey"] for m in lib.collection_rows(tag["id"])])
        loner = next(it for it in lib.items.values()
                     if it["type"] == "movie" and not it.get("Collection"))
        hubs = get(pms, f"/library/metadata/{loner['ratingKey']}/related")["Hub"]
        self.assertFalse(any(h["hubIdentifier"].startswith("collection.related") for h in hubs))

    def test_default_generated_data_is_stable(self):
        # #266 added canNormalizeLoudness to every generated audio stream and five fixed
        # enhancement fixture movies (own section id, invisible to sections 1/2) to every
        # Library — both hashes moved when that landed.
        hashes = {
            1: "c900e01d9e26ead0c63f0e2cfbf5bfad1cf410ba9a047ca701f41806246e2564",
            7: "e59fb64f00c787449263070404aed8060e07bcf644ebcc3a2184626e416d6616",
        }
        for seed, expected in hashes.items():
            payload = json.dumps(Library(seed=seed).__dict__, sort_keys=True).encode()
            self.assertEqual(hashlib.sha256(payload).hexdigest(), expected)

    def test_default_letter_counts_match_the_actual_section(self):
        pms = MockPms(Library())
        for section, count in ((1, 48), (2, 6)):
            with self.subTest(section=section):
                letters = get(pms, f"/library/sections/{section}/firstCharacter")["Directory"]
                self.assertEqual(letters, [{"key": "s", "title": "S", "size": count}])

    def test_empty_section_has_no_letter_stops(self):
        pms = MockPms(Library(movies=0))
        self.assertEqual(get(pms, "/library/sections/1/firstCharacter")["Directory"], [])

    def test_rail_buckets_cover_real_page_boundaries_and_only_their_section(self):
        pms = MockPms(Library(movies=321, rail_fixture=True))
        rows = []
        for start in range(0, 321, 60):
            page = get(pms, f"/library/sections/1/all?X-Plex-Container-Start={start}&X-Plex-Container-Size=60")
            self.assertEqual(page["totalSize"], 321)
            self.assertEqual(page["offset"], start)
            self.assertEqual(page["size"], min(60, 321 - start))
            rows.extend(page["Metadata"])
        self.assertEqual(len({row["ratingKey"] for row in rows}), 321)
        self.assertEqual([r["titleSort"] for r in rows], sorted(r["titleSort"] for r in rows))
        self.assertTrue(all(re.fullmatch(r"s[0-9a-f]{8}", r["title"]) for r in rows))
        letters = get(pms, "/library/sections/1/firstCharacter")["Directory"]
        self.assertEqual([r["title"] for r in letters], list("ACFMZ"))
        self.assertEqual(sum(r["size"] for r in letters), 321)
        offset = 0
        for letter in letters:
            prefix = letter["title"] + " "
            size = letter["size"]
            self.assertGreater(size, 60)
            self.assertTrue(all(r["titleSort"].startswith(prefix) for r in rows[offset:offset + size]))
            first = get(pms, f"/library/sections/1/all?X-Plex-Container-Start={offset}&X-Plex-Container-Size=1")["Metadata"][0]
            self.assertEqual(first["ratingKey"], rows[offset]["ratingKey"])
            offset += size
        self.assertEqual(sum(r["size"] for r in get(pms, "/library/sections/2/firstCharacter")["Directory"]), 6)

    def test_count_cannot_overlap_the_show_key_namespace(self):
        for count in (-1, 1001):
            with self.assertRaises(ValueError):
                Library(movies=count)
        library = Library(movies=1000, rail_fixture=True)
        self.assertEqual(len(library.section_items("1", {})), 1000)
        self.assertEqual(len(library.section_items("2", {})), 6)

    def test_new_sort_titles_and_letter_labels_stay_in_the_closed_alphabet(self):
        alphabet = json.loads((pathlib.Path(__file__).parent / "fixtures/replay/ALPHABET.json").read_text())
        def allowed(text):
            return text in alphabet["literals"] or any(re.fullmatch(p, text) for p in alphabet["patterns"])
        lib = Library(rail_fixture=True)
        for section in ("1", "2"):
            for row in lib.section_items(section, {}):
                self.assertTrue(allowed(row["titleSort"]))
            for row in lib.first_characters(section):
                self.assertTrue(allowed(row["key"]))
                self.assertTrue(allowed(row["title"]))
        for rejected in ("A The Godfather", "A s123456789", "B s12345678", "A s12345678 token=secret"):
            self.assertFalse(allowed(rejected), rejected)

    def test_serve_exposes_opt_in_rail_data_over_the_real_http_handler(self):
        server, _ = serve(0, movies=121, rail_fixture=True)
        try:
            base = f"http://127.0.0.1:{server.server_address[1]}"
            with urllib.request.urlopen(base + "/library/sections/1/firstCharacter", timeout=5) as response:
                rows = json.load(response)["MediaContainer"]["Directory"]
            self.assertEqual([r["title"] for r in rows], list("ACFMZ"))
            self.assertEqual(sum(r["size"] for r in rows), 121)
            with urllib.request.urlopen(base + "/library/sections/1/all?X-Plex-Container-Start=120&X-Plex-Container-Size=60", timeout=5) as response:
                page = json.load(response)["MediaContainer"]
            self.assertEqual((page["offset"], page["size"], page["totalSize"]), (120, 1, 121))
        finally:
            server.shutdown()
            server.server_close()


class LetterRailFilters(unittest.TestCase):
    """`firstCharacter` takes the same terms as `/all` (docs/pms-api.md, "Letter index"): its counts
    describe the filtered listing, so the app's jump offsets (prefix sums) land on the right title."""

    def counts(self, pms, section, terms):
        query = f"?{terms}" if terms else ""
        return get(pms, f"/library/sections/{section}/firstCharacter{query}")["Directory"]

    def total(self, pms, section, terms):
        joined = (terms + "&" if terms else "")
        return get(pms, f"/library/sections/{section}/all?{joined}"
                        "X-Plex-Container-Start=0&X-Plex-Container-Size=0")["totalSize"]

    def assert_offsets_land_on_their_letter(self, pms, section, terms):
        """Sum == totalSize, and the item at each prefix-summed offset starts that letter."""
        letters = self.counts(pms, section, terms)
        self.assertEqual(sum(r["size"] for r in letters), self.total(pms, section, terms), terms)
        joined = (terms + "&" if terms else "")
        offset = 0
        for letter in letters:
            first = get(pms, f"/library/sections/{section}/all?{joined}sort=titleSort"
                             f"&X-Plex-Container-Start={offset}&X-Plex-Container-Size=1")["Metadata"][0]
            self.assertTrue(first["titleSort"].startswith(letter["title"] + " "), (terms, letter, offset))
            offset += letter["size"]
        return letters

    def test_counts_follow_unwatched_genre_and_both_for_movies(self):
        pms = MockPms(Library(movies=60, rail_fixture=True))
        genre = next(iter(pms.lib.genres.values()))["id"]
        plain = self.assert_offsets_land_on_their_letter(pms, 1, "")
        for terms in ("unwatched=1", f"genre={genre}", f"unwatched=1&genre={genre}"):
            with self.subTest(terms=terms):
                filtered = self.assert_offsets_land_on_their_letter(pms, 1, terms)
                self.assertLess(sum(r["size"] for r in filtered), sum(r["size"] for r in plain))
        by_letter = {r["title"]: r["size"] for r in self.counts(pms, 1, "unwatched=1")}
        self.assertTrue(any(by_letter.get(r["title"], 0) < r["size"] for r in plain),
                        "the fixture has watched titles, so some letter shrinks")

    def test_shows_are_scoped_by_type_and_unwatched_leaves(self):
        lib = Library(movies=0, rail_fixture=True)
        show = next(it for it in lib.items.values() if it["type"] == "show")
        for episode in lib.leaves(show["ratingKey"] if isinstance(show["ratingKey"], int) else int(show["ratingKey"])):
            episode["viewCount"] = 1
        lib._roll_up()
        pms = MockPms(lib)
        everything = self.counts(pms, 2, "type=2")
        for terms in ("type=2&unwatchedLeaves=1", "type=2&unwatched=1"):
            with self.subTest(terms=terms):
                filtered = self.assert_offsets_land_on_their_letter(pms, 2, terms)
                self.assertLess(sum(r["size"] for r in filtered), sum(r["size"] for r in everything),
                                "the fixture has a fully watched show")

    def test_a_filter_that_matches_nothing_answers_an_empty_directory(self):
        pms = MockPms(Library(movies=60, rail_fixture=True))
        self.assertEqual(self.counts(pms, 1, "genre=999999"), [])
        self.assertEqual(self.total(pms, 1, "genre=999999"), 0)

    def test_collections_keep_their_own_counts(self):
        pms = MockPms(Library())
        with_unwatched = self.counts(pms, 1, "type=18&unwatched=1")
        self.assertEqual(with_unwatched, self.counts(pms, 1, "type=18"))


# --- the server's facets (docs/library-filters-spec.md) -------------------------------------------

MOVIE_FACETS = [
    "genre", "year", "decade", "contentRating", "collection", "director", "actor", "writer",
    "producer", "country", "studio", "resolution", "hdr", "dovi", "hdr10plus", "atmos", "unwatched",
    "inProgress", "unmatched", "videoCodec", "audioCodec", "subtitleCodec", "audioLayout",
    "audioLanguage", "subtitleLanguage", "editionTitle", "label", "location"]
SHOW_FACETS = [
    "genre", "year", "contentRating", "studio", "network", "country", "collection", "director",
    "actor", "writer", "producer", "unwatchedLeaves", "unmatched", "editionTitle", "label"]
EPISODE_FACETS = [
    "year", "collection", "resolution", "hdr", "dovi", "hdr10plus", "atmos", "unwatched",
    "inProgress", "videoCodec", "audioCodec", "subtitleCodec", "audioLayout", "audioLanguage",
    "subtitleLanguage", "unmatched", "location"]
BOOLEAN_FACETS = ("hdr", "dovi", "hdr10plus", "atmos", "unwatched", "unwatchedLeaves", "inProgress",
                  "unmatched")
INTEGER_FACETS = ("year", "decade")
# The server's own `key` carries `?type=N` on most facets of a TV section, not on all (PMS 1.43.5).
SHOW_UNTYPED = ("country", "unmatched", "editionTitle")
EPISODE_TYPED = ("year", "collection", "resolution", "hdr", "dovi", "hdr10plus", "atmos",
                 "unwatched", "inProgress")
MOVIE_SORTS = ["titleSort", "addedAt", "originallyAvailableAt", "rating", "lastViewedAt", "random"]


def meta(pms, section, type_arg=None):
    suffix = f"&type={type_arg}" if type_arg else ""
    return get(pms, f"/library/sections/{section}/all?includeMeta=1{suffix}"
                    "&X-Plex-Container-Start=0&X-Plex-Container-Size=0")["Meta"]


def type_entry(meta_block, kind):
    return next((t for t in meta_block.get("Type", []) if t["type"] == kind), {})


def listing_keys(pms, section, terms=""):
    """The ratingKeys `/all?{terms}` lists, unpaged."""
    query = f"?{terms}" if terms else ""
    return [row["ratingKey"] for row in get(pms, f"/library/sections/{section}/all{query}")["Metadata"]]


def values(pms, section, facet, terms=""):
    """The rows of a facet's value list (absent `Directory` is an empty list, as PMS answers one)."""
    query = f"?{terms}" if terms else ""
    return get(pms, f"/library/sections/{section}/{facet}{query}").get("Directory", [])


class FacetMeta(unittest.TestCase):
    """`includeMeta=1` carries the server's `Filter[]` per type, as measured on PMS 1.43.5: movies
    28, shows 15, seasons none, episodes 17, each `{filter, filterType, key, title, type}`."""

    def test_a_movie_section_advertises_28_facets_in_the_servers_order(self):
        facets = type_entry(meta(MockPms(Library()), 1, 1), "movie").get("Filter", [])
        self.assertEqual([f["filter"] for f in facets], MOVIE_FACETS)
        for f in facets:
            with self.subTest(facet=f["filter"]):
                self.assertEqual(set(f), {"filter", "filterType", "key", "title", "type"})
                self.assertEqual(f["type"], "filter")
                expect = ("boolean" if f["filter"] in BOOLEAN_FACETS
                          else "integer" if f["filter"] in INTEGER_FACETS else "string")
                self.assertEqual(f["filterType"], expect)
                self.assertEqual(f["key"], f"/library/sections/1/{f['filter']}")
                self.assertTrue(f["title"].strip())

    def test_a_tv_section_has_a_type_entry_for_shows_seasons_and_episodes(self):
        m = meta(MockPms(Library()), 2, 2)
        self.assertEqual([t["type"] for t in m["Type"]], ["show", "season", "episode"])
        shows = type_entry(m, "show")
        self.assertEqual([f["filter"] for f in shows.get("Filter", [])], SHOW_FACETS)
        for f in shows["Filter"]:
            want = f"/library/sections/2/{f['filter']}" + ("" if f["filter"] in SHOW_UNTYPED else "?type=2")
            self.assertEqual(f["key"], want, f["filter"])
        self.assertEqual(type_entry(m, "season").get("Filter", []), [], "a season advertises none")
        episodes = type_entry(m, "episode")
        self.assertEqual([f["filter"] for f in episodes.get("Filter", [])], EPISODE_FACETS)
        for f in episodes["Filter"]:
            want = f"/library/sections/2/{f['filter']}" + ("?type=4" if f["filter"] in EPISODE_TYPED else "")
            self.assertEqual(f["key"], want, f["filter"])

    def test_the_active_type_follows_the_request(self):
        pms = MockPms(Library())
        active = lambda block: [t["type"] for t in block["Type"] if t.get("active")]
        self.assertEqual(active(meta(pms, 2)), ["show"], "a TV section opens on its shows")
        for arg, kind in ((2, "show"), (3, "season"), (4, "episode")):
            with self.subTest(type=arg):
                self.assertEqual(active(meta(pms, 2, arg)), [kind])
        self.assertEqual(active(meta(pms, 1)), ["movie"])

    def test_every_type_with_filters_names_its_fields_and_the_operators(self):
        m = meta(MockPms(Library()), 2, 2)
        for kind, prefix in (("show", "show."), ("episode", "episode.")):
            fields = type_entry(m, kind).get("Field", [])
            self.assertTrue(fields, kind)
            self.assertTrue(all(f["key"].startswith(prefix) for f in fields), kind)
        ops = {ft["type"]: [o["key"] for o in ft["Operator"]] for ft in m.get("FieldType", [])}
        self.assertEqual(ops.get("integer"), ["=", "!=", ">>=", "<<="])
        self.assertEqual(ops.get("tag"), ["=", "!="])
        self.assertEqual(ops.get("boolean"), ["=", "!="])
        self.assertEqual(ops.get("resolution"), ["="], "resolution has no negation")
        movie = type_entry(meta(MockPms(Library()), 1, 1), "movie")
        self.assertIn("genre", [f["key"] for f in movie.get("Field", [])])

    def test_another_movie_section_advertises_the_movie_list(self):
        """PMS's "Other videos" (section type movie, no agent) advertises the same names."""
        lib = Library()
        lib.sections.append({"key": "3", "type": "movie", "title": "s00000003", "uuid": "s00000004"})
        facets = type_entry(meta(MockPms(lib), 3, 1), "movie").get("Filter", [])
        self.assertEqual([f["filter"] for f in facets], MOVIE_FACETS)
        self.assertEqual(facets[0]["key"], "/library/sections/3/genre")

    def test_the_sorts_and_the_collection_menu_do_not_move(self):
        pms = MockPms(Library())
        self.assertEqual([s["key"] for s in type_entry(meta(pms, 1, 1), "movie").get("Sort", [])],
                         MOVIE_SORTS)
        self.assertEqual([s["key"] for s in type_entry(meta(pms, 2, 2), "show").get("Sort", [])],
                         MOVIE_SORTS, "the sim's shows keep the sorts they always had")
        kinds = meta(pms, 1, 18)["Type"]
        self.assertEqual([k["type"] for k in kinds], ["collection"])
        self.assertNotIn("Filter", kinds[0])

    def test_titles_are_the_servers_english_constants(self):
        facets = {f["filter"]: f["title"]
                  for f in type_entry(meta(MockPms(Library()), 1, 1), "movie").get("Filter", [])}
        self.assertEqual({k: facets.get(k) for k in ("dovi", "inProgress", "location", "hdr10plus")},
                         {"dovi": "DOVI", "inProgress": "In Progress", "location": "Folder Location",
                          "hdr10plus": "HDR10+"})
        shows = {f["filter"]: f["title"]
                 for f in type_entry(meta(MockPms(Library()), 2, 2), "show").get("Filter", [])}
        self.assertEqual(shows.get("unwatchedLeaves"), "Unwatched")


class FacetValues(unittest.TestCase):
    """`GET /library/sections/{k}/{facet}`: the value list of a facet (docs/pms-api.md, section 2b)."""

    def setUp(self):
        self.lib = Library(movies=120, people=300)
        self.pms = MockPms(self.lib)

    def movie_items(self):
        return [it for it in self.lib.items.values() if it["type"] == "movie"
                and it["librarySectionID"] == 1]

    def test_the_people_knob_sets_the_pool_and_the_default_does_not_move(self):
        self.assertEqual(len(self.lib.people), 300)
        self.assertEqual(len(Library().people), 20)
        self.assertEqual(json.dumps(Library(people=20).__dict__, sort_keys=True),
                         json.dumps(Library().__dict__, sort_keys=True))

    def test_a_long_list_pages_like_the_listing(self):
        every = self.pms.handle("GET", "/library/sections/1/actor")
        unpaged = json.loads(every[2])["MediaContainer"]
        rows = unpaged.get("Directory", [])
        self.assertGreater(len(rows), 100, "the fixture's actor list is longer than one page")
        self.assertNotIn("totalSize", unpaged)
        self.assertNotIn("offset", unpaged)
        self.assertEqual(unpaged["size"], len(rows))
        cast = {str(t["id"]) for it in self.movie_items() for t in it["Role"]}
        self.assertEqual({r["key"] for r in rows}, cast)
        stitched = []
        for start in range(0, len(rows) + 100, 100):
            page = fetch(self.pms, *window("/library/sections/1/actor", start, 100, headers=bool(start % 200)))
            self.assertEqual(page["totalSize"], len(rows))
            self.assertEqual(page["offset"], start)
            self.assertEqual(page["size"], len(page.get("Directory", [])))
            stitched += page.get("Directory", [])
        self.assertEqual(stitched, rows, "windows are contiguous slices of the unpaged list")

    def test_size_zero_reads_the_count_and_a_start_past_the_end_is_empty(self):
        total = len(values(self.pms, 1, "actor"))
        self.assertGreater(total, 100)
        count = fetch(self.pms, window("/library/sections/1/actor", 0, 0)[0])
        self.assertEqual((count.get("totalSize"), count.get("offset"), count["size"]), (total, 0, 0))
        self.assertEqual(count.get("Directory", []), [])
        past = fetch(self.pms, window("/library/sections/1/actor", 5000, 10)[0])
        self.assertEqual((past.get("totalSize"), past.get("offset"), past["size"]), (total, 5000, 0))
        self.assertEqual(past.get("Directory", []), [])
        ignored = fetch(self.pms, "/library/sections/1/actor?X-Plex-Container-Size=5")
        self.assertEqual(len(ignored["Directory"]), total, "a Size without a Start is ignored")
        self.assertNotIn("totalSize", ignored)

    def test_rows_carry_what_the_server_puts_on_them(self):
        for facet, fast, thumb in (("genre", True, False), ("year", True, False), ("decade", True, False),
                                   ("actor", True, True), ("director", True, True),
                                   ("contentRating", True, False), ("resolution", False, False),
                                   ("videoCodec", False, False), ("audioLayout", False, False),
                                   ("audioLanguage", False, False), ("subtitleLanguage", False, False)):
            rows = values(self.pms, 1, facet)
            with self.subTest(facet=facet):
                self.assertTrue(rows, "the facet has values in the generated library")
                for row in rows:
                    self.assertEqual("fastKey" in row, fast)
                    self.assertEqual("thumb" in row, thumb)
                    self.assertTrue(row["key"] and row["title"])
        row = values(self.pms, 1, "genre")[0]
        self.assertEqual(row["fastKey"], f"/library/sections/1/all?genre={row['key']}")

    def test_year_and_decade_come_newest_first(self):
        years = sorted({it["year"] for it in self.movie_items()}, reverse=True)
        self.assertEqual([r["key"] for r in values(self.pms, 1, "year")], [str(y) for y in years])
        self.assertEqual([r["title"] for r in values(self.pms, 1, "year")], [str(y) for y in years])
        decades = sorted({y // 10 * 10 for y in years}, reverse=True)
        rows = values(self.pms, 1, "decade")
        self.assertEqual([r["key"] for r in rows], [str(d) for d in decades])
        self.assertEqual([r["title"] for r in rows], [f"{d}s" for d in decades])

    def test_a_facet_with_no_values_answers_an_empty_directory_without_the_key(self):
        for facet in ("label", "editionTitle"):
            with self.subTest(facet=facet):
                container = get(self.pms, f"/library/sections/1/{facet}")
                self.assertEqual(container["size"], 0)
                self.assertNotIn("Directory", container)

    def test_a_boolean_facets_key_is_not_a_value_list(self):
        """`hdr`, `dovi`, `atmos`, `inProgress` and `unmatched` answer 404; `unwatched` answers the
        whole LISTING (99 movies, 248 KB on PMS 1.43.5), so a client that GETs it downloads a listing."""
        for facet in ("hdr", "dovi", "hdr10plus", "atmos", "inProgress", "unmatched"):
            with self.subTest(facet=facet):
                self.assertEqual(self.pms.handle("GET", f"/library/sections/1/{facet}")[0], 404)
        listing = get(self.pms, "/library/sections/1/unwatched")
        self.assertNotIn("Directory", listing)
        self.assertEqual([m["ratingKey"] for m in listing["Metadata"]],
                         listing_keys(self.pms, 1, "unwatched=1"))

    def test_keys_are_percent_encoded_and_match_after_one_more_decode(self):
        movies = sorted(self.movie_items(), key=lambda it: int(it["ratingKey"]))
        movies[0]["contentRating"] = "Not Rated"
        movies[1]["studio"] = "X Y"
        rated = values(self.pms, 1, "contentRating")
        self.assertIn("Not%20Rated", [r["key"] for r in rated])
        self.assertIn(movies[0]["ratingKey"], listing_keys(self.pms, 1, "contentRating=Not%20Rated"))
        # PMS reads the value after the HTTP decode AND once more (measured on 1.43.5: one extra
        # encoding of `Not%20Rated` still matches, two do not).
        self.assertIn(movies[0]["ratingKey"], listing_keys(self.pms, 1, "contentRating=Not%2520Rated"))
        self.assertEqual(listing_keys(self.pms, 1, "contentRating=Not%252520Rated"), [])
        studios = [r["key"] for r in values(self.pms, 1, "studio")]
        self.assertIn("X%2520Y", studios, "a studio key is a name encoded twice")
        self.assertEqual(listing_keys(self.pms, 1, "studio=X%2520Y"), [movies[1]["ratingKey"]])
        self.assertEqual(listing_keys(self.pms, 1, "studio=X%20Y"), [movies[1]["ratingKey"]],
                         "the key decoded once matches as well")
        self.assertEqual(listing_keys(self.pms, 1, "studio=X%252520Y"), [], "a key encoded again matches nothing")
        layouts = [r["key"] for r in values(self.pms, 1, "audioLayout")]
        self.assertIn("5%2E1", layouts)
        self.assertTrue(listing_keys(self.pms, 1, "audioLayout=5%2E1"))
        self.assertEqual(listing_keys(self.pms, 1, "audioLayout=5%2E1"), listing_keys(self.pms, 1, "audioLayout=5.1"))
        self.assertEqual(listing_keys(self.pms, 1, "audioLayout=5%252E1"), [],
                         "the layout is compared after the HTTP decode only")

    def test_a_single_encoded_comma_splits_and_a_double_encoded_one_does_not(self):
        """PMS decodes twice: `%2C` is the separator after the first decode, `%252C` a literal comma."""
        movie = next(it for it in self.movie_items())
        movie["studio"] = "A,B"
        self.assertIn("A%252CB", [r["key"] for r in values(self.pms, 1, "studio")])
        self.assertEqual(listing_keys(self.pms, 1, "studio=A%252CB"), [movie["ratingKey"]])
        self.assertEqual(listing_keys(self.pms, 1, "studio=A%2CB"), [], "two values, A and B")

    def test_a_list_asked_with_terms_is_scoped_by_them_and_by_its_own_facet(self):
        bare = values(self.pms, 1, "actor")
        unwatched = values(self.pms, 1, "actor", "unwatched=1")
        self.assertTrue({r["key"] for r in unwatched} < {r["key"] for r in bare})
        genre = values(self.pms, 1, "genre")[0]["key"]
        years = {str(it["year"]) for it in self.movie_items()
                 if any(str(t["id"]) == genre for t in it["Genre"])}
        self.assertEqual({r["key"] for r in values(self.pms, 1, "year", f"genre={genre}")}, years)
        together = {str(t["id"]) for it in self.movie_items()
                    if any(str(x["id"]) == genre for x in it["Genre"]) for t in it["Genre"]}
        self.assertEqual({r["key"] for r in values(self.pms, 1, "genre", f"genre={genre}")}, together)

    def test_an_untyped_list_in_a_tv_section_spans_its_types(self):
        shows = {r["key"] for r in values(self.pms, 2, "year", "type=2")}
        episodes = {r["key"] for r in values(self.pms, 2, "year", "type=4")}
        self.assertEqual({r["key"] for r in values(self.pms, 2, "year")}, shows | episodes)
        self.assertEqual(values(self.pms, 2, "director", "type=2"), [], "directors hang off episodes")
        self.assertTrue(values(self.pms, 2, "director", "type=4"))
        self.assertTrue(values(self.pms, 2, "director"))


class FacetComposition(unittest.TestCase):
    """Terms compose as PMS composes them: AND across facets (and across a repeated parameter), OR
    within one comma-joined value, negation, an unknown name ignored."""

    def setUp(self):
        self.pms = MockPms(Library(movies=120, people=60))
        self.lib = self.pms.lib
        self.genres = [r["key"] for r in values(self.pms, 1, "genre")]

    def ids(self, terms, section=1):
        return set(listing_keys(self.pms, section, terms))

    def test_a_comma_is_or_and_an_encoded_comma_is_the_same(self):
        a, b = self.genres[:2]
        either = self.ids(f"genre={a},{b}")
        self.assertEqual(either, self.ids(f"genre={a}") | self.ids(f"genre={b}"))
        self.assertGreater(len(either), len(self.ids(f"genre={a}")))
        self.assertEqual(self.ids(f"genre={a}%2C{b}"), either)

    def test_facets_and_repeated_parameters_are_and(self):
        a, b = self.genres[:2]
        self.assertEqual(self.ids(f"genre={a}&resolution=4k"), self.ids(f"genre={a}") & self.ids("resolution=4k"))
        self.assertTrue(self.ids(f"genre={a}&resolution=4k"))
        both = self.ids(f"genre={a}&genre={b}")
        self.assertEqual(both, self.ids(f"genre={a}") & self.ids(f"genre={b}"))
        self.assertLess(both, self.ids(f"genre={a},{b}"))

    def test_three_facets_intersect(self):
        a = self.genres[0]
        want = self.ids("unwatched=1") & self.ids(f"genre={a}") & self.ids("resolution=4k")
        self.assertEqual(self.ids(f"unwatched=1&genre={a}&resolution=4k"), want)
        self.assertTrue(want, "the generated library has such a title")
        self.assertTrue(self.ids("resolution=4k&hdr=1") < self.ids("resolution=4k"))
        self.assertTrue(self.ids("resolution=4k&hdr=1"))

    def test_an_unknown_name_is_ignored_and_a_bogus_value_matches_nothing(self):
        everything = self.ids("")
        self.assertEqual(self.ids("foo=1"), everything)
        self.assertEqual(self.ids("genre=99999999"), set())
        self.assertEqual(self.ids("resolution=nosuch"), set())

    def test_negation_is_the_complement_and_a_resolution_cannot_be_negated(self):
        everything = self.ids("")
        a, b = self.genres[:2]
        self.assertEqual(self.ids(f"genre!={a}"), everything - self.ids(f"genre={a}"))
        self.assertEqual(self.ids(f"genre!={a},{b}"), everything - self.ids(f"genre={a},{b}"))
        self.assertEqual(self.ids("resolution!=4k"), self.ids("resolution=4k"),
                         "resolution declares `=` only, so the server reads `!=` as `=`")

    def test_a_range_on_an_integer_facet(self):
        later = {k for k in self.ids("") if self.lib.items[int(k)]["year"] > 2010}
        self.assertEqual(self.ids("year%3E%3E=2010"), later)
        earlier = {k for k in self.ids("") if self.lib.items[int(k)]["year"] < 2010}
        self.assertEqual(self.ids("year%3C%3C=2010"), earlier)

    def test_unwatched_and_unwatched_leaves_are_two_predicates(self):
        movies = self.ids("unwatchedLeaves=1")
        self.assertEqual(movies, set(), "a movie has no leaves")
        lib = Library(movies=0, caption_edge_cases=True)
        pms = MockPms(lib)
        show = next(it for it in lib.items.values() if it["type"] == "show" and it["leafCount"] == 0)
        own = set(listing_keys(pms, 2, "type=2&unwatched=1"))
        leaves = set(listing_keys(pms, 2, "type=2&unwatchedLeaves=1"))
        self.assertIn(show["ratingKey"], own, "a show with no episodes has no watched state of its own")
        self.assertNotIn(show["ratingKey"], leaves, "and no unwatched leaf")
        self.assertTrue(leaves < own)

    def test_a_trait_the_type_does_not_advertise_is_evaluated_unpredictably_but_answers(self):
        """PMS evaluates `hdr=1` on shows through their episodes and answers 0 for `genre` on episodes."""
        shows = set(listing_keys(self.pms, 2, "type=2&hdr=1"))
        episodes = set(listing_keys(self.pms, 2, "type=4&hdr=1"))
        self.assertTrue(shows)
        self.assertEqual(shows, {self.lib.items[int(k)]["grandparentRatingKey"] for k in episodes})
        genre = self.genres[0]
        self.assertEqual(self.ids(f"type=4&genre={genre}", 2), set())

    def test_the_people_facets_match_a_tag_id_in_any_people_role(self):
        movie = next(it for it in self.lib.items.values() if it["type"] == "movie")
        director = str(movie["Director"][0]["id"])
        self.assertTrue(self.ids(f"director={director}"))
        self.assertEqual(self.ids(f"director={director}"), self.ids(f"actor={director}"))
        self.assertEqual(self.ids(f"director={director}"), self.ids(f"writer={director}"))


class FacetsAndTheRail(unittest.TestCase):
    """`firstCharacter` honours every facet: its counts sum to the filtered `totalSize` and its prefix
    sums land on the first title of each letter (24 of 24 combinations on PMS 1.43.5)."""

    def check(self, pms, section, terms):
        """Sum == the filtered `totalSize`, and the title at each prefix-summed offset starts its
        letter (the first character of its sort title, so it holds for episodes too)."""
        joined = (terms + "&" if terms else "")
        letters = get(pms, f"/library/sections/{section}/firstCharacter" + (f"?{terms}" if terms else ""))["Directory"]
        total = get(pms, f"/library/sections/{section}/all?{joined}"
                         "X-Plex-Container-Start=0&X-Plex-Container-Size=0")["totalSize"]
        self.assertEqual(sum(r["size"] for r in letters), total, terms)
        offset = 0
        for letter in letters:
            first = get(pms, f"/library/sections/{section}/all?{joined}sort=titleSort"
                             f"&X-Plex-Container-Start={offset}&X-Plex-Container-Size=1")["Metadata"][0]
            self.assertEqual(first["titleSort"][0].upper(), letter["title"], (terms, letter, offset))
            offset += letter["size"]
        return total

    def test_every_term_scopes_the_counts_like_the_listing(self):
        pms = MockPms(Library(movies=120, people=60, rail_fixture=True))
        genres = [r["key"] for r in values(pms, 1, "genre")][:2]
        everything = self.check(pms, 1, "")
        for terms in ("resolution=4k", "hdr=1", "dovi=1", "atmos=1", "inProgress=1", "unmatched=1",
                      f"genre={genres[0]},{genres[1]}", f"genre!={genres[0]}", "year%3E%3E=2000",
                      "audioLayout=5%2E1", "videoCodec=hevc", "audioLanguage=de", "subtitleLanguage=fr",
                      f"genre={genres[0]},{genres[1]}&resolution=4k&hdr=1"):
            with self.subTest(terms=terms):
                self.assertLess(self.check(pms, 1, terms), everything)
        self.assertLess(self.check(pms, 1, "resolution=4k&hdr=1"), self.check(pms, 1, "resolution=4k"))

    def test_shows_and_episodes_follow_their_own_facets(self):
        pms = MockPms(Library(movies=0, rail_fixture=True))
        for terms, base in (("type=2&unmatched=1", "type=2"),
                            ("type=4&hdr=1", "type=4"), ("type=4&unwatched=1", "type=4"),
                            ("type=4&resolution=4k", "type=4")):
            with self.subTest(terms=terms):
                self.assertLess(self.check(pms, 2, terms), self.check(pms, 2, base))


class ResumeFixture(unittest.TestCase):
    """`--resume-fixture`: a show that has been watched up to S12E14, and one that has not."""

    def test_the_on_deck_show_carries_its_episode_only_when_asked(self):
        pms = MockPms(Library(resume_fixture=True))
        show = get(pms, f"/library/metadata/{RESUME_ON_DECK_SHOW_RK}?includeOnDeck=1")["Metadata"][0]
        deck = show["OnDeck"]["Metadata"]
        self.assertEqual((deck["parentIndex"], deck["index"], deck["type"]), (12, 14, "episode"))
        self.assertGreater(deck["viewOffset"], 0)
        self.assertEqual(show["childCount"], 12)
        self.assertNotIn("OnDeck", get(pms, f"/library/metadata/{RESUME_ON_DECK_SHOW_RK}")["Metadata"][0])

    def test_the_other_show_has_no_on_deck_and_is_unwatched(self):
        pms = MockPms(Library(resume_fixture=True))
        show = get(pms, f"/library/metadata/{RESUME_PLAIN_SHOW_RK}?includeOnDeck=1")["Metadata"][0]
        self.assertNotIn("OnDeck", show)
        self.assertEqual(show["viewedLeafCount"], 0)

    def test_the_resume_episode_heads_continue_watching_and_a_season_pages_over_one_screen(self):
        pms = MockPms(Library(resume_fixture=True))
        deck = get(pms, "/hubs/continueWatching?count=12")["Hub"][0]["Metadata"]
        self.assertEqual(deck[0]["grandparentRatingKey"], str(RESUME_ON_DECK_SHOW_RK))
        seasons = get(pms, f"/library/metadata/{RESUME_ON_DECK_SHOW_RK}/children")["Metadata"]
        self.assertEqual([s["index"] for s in seasons], list(range(1, 13)))
        self.assertEqual(len(get(pms, f"/library/metadata/{seasons[11]['ratingKey']}/children")["Metadata"]), 30)

    def test_off_by_default_and_the_generated_library_does_not_move(self):
        plain, fixture = Library(), Library(resume_fixture=True)
        self.assertEqual(plain.on_deck, {})
        self.assertNotIn(RESUME_ON_DECK_SHOW_RK, plain.items)
        for rk, item in plain.items.items():
            if item["type"] in ("movie", "show", "season", "episode") and rk < 970000:
                self.assertEqual(item, {**fixture.items[rk],
                                        **{k: item[k] for k in ("leafCount", "viewedLeafCount", "childCount")
                                           if k in item}}, rk)


class CaptionEdgeCases(unittest.TestCase):
    """`--caption-edge-cases`: the items an under-poster caption falls back on, which no generated
    item is. Off by default, and then no generated byte moves."""

    def lib(self):
        return Library(caption_edge_cases=True)

    def test_a_movie_with_no_year_keeps_its_length_and_one_with_nothing_keeps_nothing(self):
        items = self.lib().items
        year_less, bare = items[CAPTION_EDGE_RK_BASE], items[CAPTION_EDGE_RK_BASE + 1]
        for it in (year_less, bare):
            self.assertEqual(it["type"], "movie")
            self.assertNotIn("year", it)
            self.assertNotIn("originallyAvailableAt", it)
        self.assertGreater(year_less["duration"], 0)
        self.assertTrue(year_less["Genre"])
        for key in ("duration", "Genre"):
            self.assertNotIn(key, bare)

    def test_the_edge_items_head_the_a_to_z_listings(self):
        pms = MockPms(self.lib())
        movies = get(pms, "/library/sections/1/all?sort=titleSort")["Metadata"]
        self.assertEqual([m["ratingKey"] for m in movies[:2]],
                         [str(CAPTION_EDGE_RK_BASE), str(CAPTION_EDGE_RK_BASE + 1)])
        shows = get(pms, "/library/sections/2/all?sort=titleSort")["Metadata"]
        self.assertEqual({s["ratingKey"] for s in shows[:2]},
                         {str(CAPTION_EDGE_RK_BASE + 2), str(CAPTION_EDGE_RK_BASE + 9)})

    def test_a_show_has_no_year_and_no_seasons(self):
        show = self.lib().items[CAPTION_EDGE_RK_BASE + 2]
        self.assertEqual((show["type"], show["childCount"]), ("show", 0))
        self.assertNotIn("year", show)

    def test_a_show_lists_an_untitled_season_zero_before_season_one_and_a_numberless_episode(self):
        pms = MockPms(self.lib())
        show_rk = CAPTION_EDGE_RK_BASE + 9
        seasons = get(pms, f"/library/metadata/{show_rk}/children")["Metadata"]
        self.assertEqual([(s["index"], s["title"]) for s in seasons][0], (0, ""))
        self.assertEqual([s["index"] for s in seasons], [0, 1])
        for season in seasons:
            eps = get(pms, f"/library/metadata/{season['ratingKey']}/children")["Metadata"]
            self.assertEqual([e.get("index") for e in eps], [None, 1], "the numberless one sorts first")
            self.assertNotIn("year", eps[0])
            self.assertNotIn("duration", eps[0])
        self.assertEqual(len(get(pms, f"/library/metadata/{show_rk}/allLeaves")["Metadata"]), 4)

    def test_two_collections_are_named_by_more_than_a_face_can_set(self):
        pms = MockPms(self.lib())
        rows = get(pms, "/library/sections/1/collections")["Metadata"]
        long = [r for r in rows if len(r["title"].split()) >= 5]
        self.assertEqual(len(long), 2)
        self.assertEqual(sorted("thumb" in r for r in long), [False, True], "a fan and a neutral tile")
        self.assertGreater(max(r["childCount"] for r in long), 0)

    def test_the_section_listings_still_answer_with_the_new_items(self):
        pms = MockPms(self.lib())
        for path in ("/library/sections/1/all?sort=year:desc", "/library/sections/1/all",
                     "/library/sections/2/all?sort=year", "/library/sections/2/all?type=4",
                     "/hubs?count=12", "/hubs/continueWatching?count=12"):
            with self.subTest(path=path):
                get(pms, path)

    def test_off_by_default_and_the_generated_library_does_not_move(self):
        plain, flagged = Library(), self.lib()
        self.assertNotIn(CAPTION_EDGE_RK_BASE, plain.items)
        self.assertEqual(len(plain.collections), 4)
        for rk, item in plain.items.items():
            kept = {k: v for k, v in flagged.items[rk].items()
                    if k not in ("Collection", "leafCount", "viewedLeafCount", "childCount")}
            self.assertEqual({k: v for k, v in item.items()
                              if k not in ("Collection", "leafCount", "viewedLeafCount", "childCount")},
                             kept, rk)


class HomeHubsFlag(unittest.TestCase):
    """#395: `--home-hubs N` makes `/hubs` answer exactly N hubs, so the app's Home row cap can be
    exercised at 18 and 170 rows."""
    PATH = "/hubs?count=12&excludeContinueWatching=1"

    def test_home_hubs_flag_serves_the_requested_number_of_hubs(self):
        pms = MockPms(Library())
        pms.home_hubs = 18
        hubs = get(pms, self.PATH)["Hub"]
        self.assertEqual(len(hubs), 18)
        for h in hubs[1:]:
            self.assertTrue(h["Metadata"])
            for it in h["Metadata"]:
                self.assertTrue(it["title"] and it["thumb"])
            self.assertEqual(h["size"], len(h["Metadata"]))
        self.assertEqual(len({h["hubIdentifier"] for h in hubs}), 18)
        self.assertEqual(len({h["title"] for h in hubs}), 18)
        shelf = next(h for h in hubs if h["hubIdentifier"] == "mock.shelf.3")
        self.assertEqual(get(pms, shelf["key"])["Metadata"][:12], shelf["Metadata"])
        self.assertLessEqual(len(get(pms, "/hubs?count=2")["Hub"][-1]["Metadata"]), 2)

    def test_home_hubs_default_leaves_the_response_unchanged(self):
        pms = MockPms(Library())
        self.assertEqual(pms.home_hubs, 0)
        before = pms.handle("GET", self.PATH)
        hubs = json.loads(before[2])["MediaContainer"]["Hub"]
        self.assertEqual([h["hubIdentifier"] for h in hubs],
                         ["home.continue", "home.movies.recent", "home.television.recent"])


class HomeSeasonsFlag(unittest.TestCase):
    """`--home-seasons` adds a Recently Added shelf of SEASON items to `/hubs`: what Home's captions
    are graded on for a tile that is not a movie, a show or an episode."""
    PATH = "/hubs?count=12&excludeContinueWatching=1"

    def test_the_shelf_holds_seasons_that_name_their_show_and_pages_through_its_key(self):
        pms = MockPms(Library(shows=3, seasons=2))
        pms.home_seasons = True
        hubs = get(pms, self.PATH)["Hub"]
        self.assertEqual([h["hubIdentifier"] for h in hubs],
                         ["home.continue", "mock.seasons", "home.movies.recent", "home.television.recent"])
        shelf = hubs[1]
        self.assertEqual(len(shelf["Metadata"]), 6)
        self.assertEqual(shelf["size"], 6)
        for it in shelf["Metadata"]:
            self.assertEqual(it["type"], "season")
            self.assertTrue(it["title"] and it["parentTitle"] and it["thumb"], it)
            self.assertGreater(it["index"], 0)
        self.assertEqual(get(pms, shelf["key"])["Metadata"][:12], shelf["Metadata"])

    def test_the_default_leaves_the_response_unchanged(self):
        pms = MockPms(Library())
        self.assertFalse(pms.home_seasons)
        hubs = json.loads(pms.handle("GET", self.PATH)[2])["MediaContainer"]["Hub"]
        self.assertEqual([h["hubIdentifier"] for h in hubs],
                         ["home.continue", "home.movies.recent", "home.television.recent"])
        self.assertFalse(get(pms, "/hubs/mock/seasons").get("Metadata"), "its key answers nothing unless asked for")

    def test_the_flag_reaches_the_server(self):
        server, pms = serve(0, home_seasons=True)
        try:
            self.assertTrue(pms.home_seasons)
        finally:
            server.shutdown()
            server.server_close()


class SectionHubsFlag(unittest.TestCase):
    """#412: `--section-hubs N` makes `/hubs/sections/<id>` answer exactly N hubs and
    `--section-hubs-linked M` makes M of them promoted collections, so the Library's shelf bound
    can be exercised at 170 shelves."""
    PATH = "/hubs/sections/1"

    @staticmethod
    def _pms(n, linked=0):
        pms = MockPms(Library())
        pms.section_hubs, pms.section_hubs_linked = n, linked
        return pms

    def test_default_leaves_the_response_unchanged(self):
        pms = MockPms(Library())
        self.assertEqual((pms.section_hubs, pms.section_hubs_linked), (0, 0))
        hubs = get(pms, self.PATH)["Hub"]
        self.assertEqual([h["hubIdentifier"] for h in hubs],
                         ["movie.inprogress.1", "movie.recentlyadded.1"])

    def test_serves_exactly_n_distinct_hubs_each_with_items(self):
        pms = self._pms(170)
        hubs = get(pms, self.PATH)["Hub"]
        self.assertEqual(len(hubs), 170)
        self.assertEqual(len({h["hubIdentifier"] for h in hubs}), 170)
        self.assertEqual(len({h["title"] for h in hubs[2:]}), 168)
        for h in hubs[2:]:
            self.assertTrue(h["hubIdentifier"].startswith("mock.section.1.shelf."))
            self.assertTrue(h["Metadata"])
            self.assertEqual(h["size"], len(h["Metadata"]))
        shelf = hubs[5]
        self.assertEqual(shelf["key"], f"/hubs/mock/section/1/shelf/{5 - 1}")
        self.assertEqual(get(pms, shelf["key"])["Metadata"][:12], shelf["Metadata"])
        # the second section pads too, with its own kind
        self.assertEqual(len(get(pms, "/hubs/sections/2")["Hub"]), 170)

    def test_linked_hubs_are_promoted_collections_the_mock_resolves(self):
        pms = self._pms(170, 40)
        hubs = get(pms, self.PATH)["Hub"]
        self.assertEqual(len(hubs), 170)
        linked = [h for h in hubs if h["hubIdentifier"].startswith("custom.collection.")]
        self.assertEqual(len(linked), 40)
        self.assertEqual(len({h["hubIdentifier"] for h in hubs}), 170)
        for h in linked:
            # the shape plx_plex's promoted_collection_link accepts: section 1, a rating key, and
            # a listing key naming that same collection
            _, _, section, rk, _ = h["hubIdentifier"].split(".")
            self.assertEqual(section, "1")
            self.assertEqual(h["key"], f"/library/collections/{rk}/children")
            self.assertTrue(h["Metadata"])
            self.assertEqual(get(pms, h["key"])["Metadata"][:len(h["Metadata"])], h["Metadata"])
        # spread through the list, not bunched at the end
        first = next(i for i, h in enumerate(hubs) if h in linked)
        self.assertLess(first, 10)

    def test_the_shipped_scene_arguments_hold(self):
        # tests/manifest.json library-shelves-deep: 170 hubs, 40 of them linked
        pms = self._pms(170, 40)
        hubs = get(pms, self.PATH)["Hub"]
        self.assertEqual(sum(h["hubIdentifier"].startswith("custom.collection.") for h in hubs), 40)

    def test_the_flags_reach_the_server_and_the_cli_refuses_nonsense(self):
        server, pms = serve(0, section_hubs=9, section_hubs_linked=3)
        try:
            self.assertEqual((pms.section_hubs, pms.section_hubs_linked), (9, 3))
        finally:
            server.shutdown()
            server.server_close()
        for argv in (["--section-hubs", "-1"], ["--section-hubs", "2", "--section-hubs-linked", "3"]):
            p = subprocess.run(["python3", str(pathlib.Path(__file__).with_name("mock_pms.py")),
                                "--port", "0", *argv], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(p.returncode, 0, argv)


class DoviAndExtraMedia(unittest.TestCase):
    """The DOVI*/container derivation `--extra-media` relies on — the field NAMES a real PMS
    sends (docs/pms-api.md, verified live 2026-08-21), mapped from ffprobe's own "DOVI
    configuration record" side-data shape rather than probed against a real DV file here."""

    def test_dovi_wire_maps_the_real_pms_field_names(self):
        stream = {
            "side_data_list": [{
                "side_data_type": "DOVI configuration record",
                "dv_version_major": 1, "dv_version_minor": 0,
                "dv_profile": 8, "dv_level": 6,
                "rpu_present_flag": 1, "el_present_flag": 0, "bl_present_flag": 1,
                "dv_bl_signal_compatibility_id": 1,
            }],
        }
        self.assertEqual(Library._dovi_wire(stream), {
            "DOVIPresent": True, "DOVIProfile": 8, "DOVIBLCompatID": 1,
            "DOVIELPresent": False, "DOVILevel": 6, "DOVIVersion": "1.0",
            "DOVIBLPresent": True, "DOVIRPUPresent": True,
        })

    def test_dovi_wire_is_silent_without_a_configuration_record(self):
        # Silence must not convict (metadata::Dovi's rule): an ordinary SDR/HDR10 stream, one
        # with no side-data at all, and one whose side-data is unrelated (e.g. embedded cover
        # art) must all send NONE of the DOVI* keys rather than a false profile/compat id of 0.
        self.assertEqual(Library._dovi_wire({"side_data_list": []}), {})
        self.assertEqual(Library._dovi_wire({}), {})
        self.assertEqual(
            Library._dovi_wire({"side_data_list": [{"side_data_type": "Something Else"}]}), {})

    def test_resolution_label_buckets_by_decoded_size_not_a_hardcoded_1080p(self):
        self.assertEqual(Library._resolution_label(3840, 1604), ("4k", "4K"))
        self.assertEqual(Library._resolution_label(1920, 1080), ("1080", "1080p"))
        self.assertEqual(Library._resolution_label(1280, 720), ("720", "720p"))
        self.assertEqual(Library._resolution_label(64, 64), ("sd", "SD"))

    def test_extra_media_ids_never_collide_with_a_large_generated_library_or_the_verify_block(self):
        lib = Library(movies=1000, shows=50, rail_fixture=True)
        self.assertNotIn(EXTRA_MEDIA_RK_BASE, lib.items)
        self.assertLess(max(lib.items), EXTRA_MEDIA_RK_BASE)

    @unittest.skipUnless(
        HAS_FFMPEG_AND_FFPROBE,
        "needs ffmpeg+ffprobe; the host CI runner has neither — the field mapping is covered by the pure tests above",
    )
    def test_extra_media_container_and_content_type_come_from_the_file_extension(self):
        with tempfile.TemporaryDirectory() as tmp:
            mp4 = pathlib.Path(tmp) / "clip.mp4"
            mkv = pathlib.Path(tmp) / "clip.mkv"
            for out in (mp4, mkv):
                subprocess.check_call([
                    "ffmpeg", "-y", "-v", "error", "-f", "lavfi", "-i",
                    "color=size=64x64:rate=24:duration=1", "-f", "lavfi", "-i",
                    "sine=frequency=330:duration=1", "-c:v", "libx264", "-c:a", "aac", "-t", "1",
                    str(out)])
            server, pms = serve(0, movies=0, extra_media=[mp4, mkv])
            try:
                lib = pms.lib
                rks = sorted(lib.extra_media_files)
                self.assertEqual(rks, [EXTRA_MEDIA_RK_BASE, EXTRA_MEDIA_RK_BASE + 1])
                base = f"http://127.0.0.1:{server.server_address[1]}"
                for rk, want_container, want_ct in (
                        (rks[0], "mp4", "video/mp4"), (rks[1], "mkv", "video/x-matroska")):
                    media = lib.items[rk]["Media"][0]
                    part = media["Part"][0]
                    self.assertEqual(media["container"], want_container)
                    self.assertTrue(part["key"].endswith(f"/file.{want_container}"), part["key"])
                    self.assertEqual(lib.media_content_type[part["id"]], want_ct)
                    # the actual byte-serving path (Handler._media), not the synthetic-item
                    # fallback in MockPms.handle — this is the header the app's part probe sees.
                    with urllib.request.urlopen(base + part["key"], timeout=5) as response:
                        self.assertEqual(response.headers["Content-Type"], want_ct)
            finally:
                server.shutdown()
                server.server_close()

    @unittest.skipUnless(
        HAS_FFMPEG_AND_FFPROBE,
        "needs ffmpeg+ffprobe; the host CI runner has neither — the field mapping is covered by the pure tests above",
    )
    def test_startup_prints_ratingkey_to_filename_for_every_extra_media_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            clip = pathlib.Path(tmp) / "clip.mp4"
            subprocess.check_call([
                "ffmpeg", "-y", "-v", "error", "-f", "lavfi", "-i",
                "color=size=64x64:rate=24:duration=1", "-f", "lavfi", "-i",
                "sine=frequency=330:duration=1", "-c:v", "libx264", "-c:a", "aac", "-t", "1",
                str(clip)])
            server, pms = serve(0, movies=0, extra_media=[clip])
            try:
                self.assertEqual(pms.lib.extra_media_files, {EXTRA_MEDIA_RK_BASE: clip})
            finally:
                server.shutdown()
                server.server_close()


class PartFaults(unittest.TestCase):
    """`--cut-part-after` and `--part-range-answers-200`: opt-in faults on media part bytes, for
    reproducing a direct-play body cut (and a server that ignores Range) against the mock."""

    BODY = bytes(range(256)) * 8  # 2048 bytes

    def _serve(self, **faults):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        clip = pathlib.Path(tmp.name) / "clip.mkv"
        clip.write_bytes(self.BODY)
        server, pms = serve(0, movies=0, **faults)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        pms.lib.media_files = {77: clip}
        pms.lib.media_content_type = {77: "video/x-matroska"}
        return f"http://127.0.0.1:{server.server_address[1]}/library/parts/77/1/file.mkv"

    def _get(self, url, range_from=None):
        request = urllib.request.Request(
            url, headers={} if range_from is None else {"Range": f"bytes={range_from}-"})
        with urllib.request.urlopen(request, timeout=5) as response:
            try:
                body = response.read()
            except Exception as e:  # http.client.IncompleteRead carries what arrived
                body = getattr(e, "partial", b"")
            return response.status, response.headers.get("Content-Range"), body

    def test_off_by_default_serves_whole_and_ranged(self):
        url = self._serve()
        self.assertEqual(self._get(url), (200, None, self.BODY))
        self.assertEqual(self._get(url, 100), (206, "bytes 100-2047/2048", self.BODY[100:]))

    def test_the_first_response_of_a_part_is_cut_and_the_ranged_reopen_is_honest(self):
        url = self._serve(cut_part_after=500)
        status, _, body = self._get(url, 0)
        self.assertEqual((status, body), (206, self.BODY[:500]))
        self.assertEqual(self._get(url, 500), (206, "bytes 500-2047/2048", self.BODY[500:]))
        # cut once per part, not once per request
        self.assertEqual(self._get(url, 0)[2], self.BODY)

    def test_a_head_or_a_short_read_does_not_spend_the_cut(self):
        url = self._serve(cut_part_after=500)
        head = urllib.request.Request(url, method="HEAD")
        with urllib.request.urlopen(head, timeout=5) as response:
            self.assertEqual(response.status, 200)
        # a ranged read that ends before the cut point carries no more than BYTES bytes
        short = urllib.request.Request(url, headers={"Range": "bytes=0-99"})
        with urllib.request.urlopen(short, timeout=5) as response:
            self.assertEqual(response.read(), self.BODY[:100])
        status, _, body = self._get(url, 0)
        self.assertEqual((status, body), (206, self.BODY[:500]))

    def test_a_cut_past_the_end_of_the_body_cuts_nothing(self):
        url = self._serve(cut_part_after=5000)
        self.assertEqual(self._get(url, 0)[2], self.BODY)

    def test_range_answers_200_only_past_byte_zero(self):
        url = self._serve(part_range_answers_200=True)
        self.assertEqual(self._get(url, 0)[:2], (206, "bytes 0-2047/2048"))
        self.assertEqual(self._get(url, 500), (200, None, self.BODY))


class PlaintextOnlyLan(unittest.TestCase):
    """PLX-NATIVE-10: `--plaintext-only-lan`'s wire shape and the fail-mode/advertise-ip
    plumbing, kept separate from the (also-passing) live-socket assertions in mock_pms.py's own
    `--selftest`, which additionally proves the https route really fails a TLS handshake."""

    def test_resources_shape_has_no_relay_and_the_two_documented_connections(self):
        lib = Library(seed=7)
        rows = plaintext_only_lan_resources(lib, "127.0.0.1", 32499, 32500, "s0a0b0c0d")
        self.assertEqual(len(rows), 1)
        res = rows[0]
        self.assertEqual(res["clientIdentifier"], lib.machine)
        self.assertEqual(res["provides"], "server")
        self.assertIs(res["owned"], True)
        self.assertIs(res["httpsRequired"], False)
        self.assertIs(res["publicAddressMatches"], True)
        self.assertIsNone(res["sourceTitle"])
        conns = res["connections"]
        self.assertEqual(len(conns), 2)
        self.assertFalse(any(c["relay"] for c in conns))
        https = next(c for c in conns if c["protocol"] == "https")
        http = next(c for c in conns if c["protocol"] == "http")
        self.assertEqual(https, {"protocol": "https", "address": "127.0.0.1", "port": 32500,
                                  "uri": f"https://127-0-0-1.{PLEX_DIRECT_HASH}.plex.direct:32500",
                                  "local": True, "relay": False, "IPv6": False})
        self.assertEqual(http, {"protocol": "http", "address": "127.0.0.1", "port": 32499,
                                 "uri": "http://127.0.0.1:32499", "local": True, "relay": False,
                                 "IPv6": False})

    def test_resource_discovery_advertises_only_the_bound_mock(self):
        pms = MockPms(Library(seed=7))
        status, ctype, body = pms.handle("GET", "/api/v2/resources")
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body), [], "an unbound fixture advertises no listener")
        self.assertEqual(pms.unknown, [])
        server, pms = serve(0, seed=7)
        try:
            status, _, body = pms.handle("GET", "/api/v2/resources")
            self.assertEqual(status, 200)
            resources = json.loads(body)
            self.assertEqual(len(resources), 1)
            self.assertEqual(resources[0]["clientIdentifier"], pms.lib.machine)
            self.assertEqual(resources[0]["connections"], [{
                "protocol": "http", "address": "127.0.0.1", "port": server.server_address[1],
                "uri": f"http://127.0.0.1:{server.server_address[1]}", "local": True,
                "relay": False, "IPv6": False,
            }])
        finally:
            server.shutdown()
            server.server_close()

    def test_advertise_ip_none_falls_back_to_loopback_with_a_warning_when_undetectable(self):
        import ipaddress
        server, pms = serve(0, seed=7, plaintext_only_lan=True, advertise_ip=None)
        try:
            ipaddress.IPv4Address(pms.plaintext_only_lan["ip"])  # a real, parseable IPv4
            # a real LAN IPv4 needs no warning; the loopback fallback always gets one.
            if pms.plaintext_only_lan["ip"] == "127.0.0.1":
                buf = io.StringIO()
                with contextlib.redirect_stderr(buf):
                    server2, pms2 = serve(0, seed=7, plaintext_only_lan=True, advertise_ip=None)
                try:
                    self.assertEqual(pms2.plaintext_only_lan["ip"], "127.0.0.1")
                    self.assertIn("NOT LAN-eligible", buf.getvalue())
                finally:
                    server2.shutdown()
                    server2.server_close()
        finally:
            server.shutdown()
            server.server_close()

    def test_unreachable_fail_mode_opens_no_listener_and_refuses(self):
        server, pms = serve(0, seed=7, plaintext_only_lan=True, advertise_ip="127.0.0.1",
                            insecure_fail_mode="unreachable")
        try:
            self.assertIsNone(server.insecure_fail_listener)
            with self.assertRaises(OSError):
                socket.create_connection(
                    ("127.0.0.1", pms.plaintext_only_lan["fail_port"]), timeout=5).close()
        finally:
            server.shutdown()
            server.server_close()

    def test_identity_over_the_plaintext_connection_is_tokenless_and_matches_the_resource(self):
        server, pms = serve(0, seed=7, plaintext_only_lan=True, advertise_ip="127.0.0.1")
        try:
            base = f"http://127.0.0.1:{server.server_address[1]}"
            with urllib.request.urlopen(base + "/identity", timeout=5) as response:
                identity = json.load(response)["MediaContainer"]
            self.assertEqual(identity["machineIdentifier"], pms.lib.machine)
        finally:
            server.shutdown()
            server.server_close()


class Enhancement266(unittest.TestCase):
    """#266 (Boost Dialog / Normalize Loudness) mock support: the plex-pass/loudness-capability
    flags, the /decision enhancement shapes, the runtime /_mock/config toggle, the request log,
    and --transcode-fixture."""

    def _audio(self, item):
        return next(s for s in item["Media"][0]["Part"][0]["Stream"] if s["streamType"] == 2)

    def test_no_plex_pass_flag(self):
        pms = MockPms(Library())
        self.assertIs(get(pms, "/identity")["myPlexSubscription"], True)
        self.assertEqual(get(pms, "/identity")["version"], "1.41.0.0000-synthetic")

        server, pms2 = serve(0, seed=1, plex_pass=False)
        try:
            identity = json.loads(pms2.handle("GET", "/identity")[2])["MediaContainer"]
            self.assertIs(identity["myPlexSubscription"], False)
            # tools/mock-guest.py keys off this exact version string — untouched by the flag.
            self.assertEqual(identity["version"], "1.41.0.0000-synthetic")
        finally:
            server.shutdown()
            server.server_close()

        # also flippable live
        pms.handle("POST", "/_mock/config", json.dumps({"plex_pass": False}).encode())
        self.assertIs(get(pms, "/identity")["myPlexSubscription"], False)

    def test_loudness_attr_default_and_flag(self):
        lib_on = Library(movies=1)
        self.assertEqual(self._audio(lib_on.items[1001])["canNormalizeLoudness"], "1")
        self.assertEqual(
            self._audio(lib_on.items[ENHANCEMENT_AC3_2CH_RK])["canNormalizeLoudness"], "1")

        lib_off = Library(movies=1, loudness_analysis=False)
        self.assertNotIn("canNormalizeLoudness", self._audio(lib_off.items[1001]))
        self.assertNotIn("canNormalizeLoudness",
                         self._audio(lib_off.items[ENHANCEMENT_AC3_2CH_RK]))

    def test_fixture_items_have_the_documented_properties(self):
        lib = Library()
        ac3 = lib.items[ENHANCEMENT_AC3_2CH_RK]
        self.assertEqual((ac3["Media"][0]["audioCodec"], ac3["Media"][0]["audioChannels"]),
                         ("ac3", 2))
        aac = lib.items[ENHANCEMENT_AAC_51_RK]
        self.assertEqual((aac["Media"][0]["audioCodec"], aac["Media"][0]["audioChannels"]),
                         ("aac", 6))
        dv = lib.items[ENHANCEMENT_DV_P8_RK]
        video = next(s for s in dv["Media"][0]["Part"][0]["Stream"] if s["streamType"] == 1)
        self.assertEqual(video["DOVIProfile"], 8)
        self.assertIs(video["DOVIPresent"], True)
        default_srt = lib.items[ENHANCEMENT_DEFAULT_SRT_RK]
        sub = next(s for s in default_srt["Media"][0]["Part"][0]["Stream"] if s["streamType"] == 3)
        self.assertIs(sub["default"], True)
        self.assertIs(sub["selected"], True)
        external = lib.items[ENHANCEMENT_EXTERNAL_SRT_RK]
        ext_sub = next(s for s in external["Media"][0]["Part"][0]["Stream"]
                       if s["streamType"] == 3 and s.get("external"))
        self.assertIs(ext_sub["selected"], True)
        self.assertEqual(ext_sub["codec"], "srt")
        self.assertTrue(ext_sub["key"])
        # invisible to the ordinary section listings/rails/counts
        # A real server answers a total only to a paged request: the size-0 window is the read.
        self.assertEqual(get(MockPms(lib), "/library/sections/1/all"
                             "?X-Plex-Container-Start=0&X-Plex-Container-Size=0")["totalSize"], 48)

    def test_decision_enhancement_shapes(self):
        pms = MockPms(Library())
        rk = ENHANCEMENT_AC3_2CH_RK
        base_q = f"path=%2Flibrary%2Fmetadata%2F{rk}&directPlay=0&directStream=1"
        path = f"/video/:/transcode/universal/decision?{base_q}"

        baseline = get(pms, path)
        self.assertEqual(baseline["generalDecisionCode"], 1000)
        self.assertEqual(baseline["Metadata"], [])
        # a param of 0 is byte-identical to no param at all
        self.assertEqual(get(pms, path + "&normalizeLoudness=0"), baseline)
        self.assertEqual(get(pms, path + "&boostDialog=0&normalizeLoudness=0"), baseline)

        on = get(pms, path + "&normalizeLoudness=1")
        part = on["Metadata"][0]["Media"][0]["Part"][0]
        self.assertEqual(part["decision"], "transcode")
        video = next(s for s in part["Stream"] if s["streamType"] == 1)
        audio = next(s for s in part["Stream"] if s["streamType"] == 2)
        self.assertEqual(video["decision"], "copy")
        self.assertEqual(audio["decision"], "transcode")
        self.assertEqual(audio["codec"], "ac3")
        self.assertEqual(audio["channels"], 2, "source channel count is preserved")

        # the MDE shape with a live param mimics the same M1 outcome
        mde_q = f"path=%2Flibrary%2Fmetadata%2F{rk}&directPlay=1&directStreamAudio=1&boostDialog=1"
        mde = get(pms, f"/video/:/transcode/universal/decision?{mde_q}")
        mde_part = mde["Metadata"][0]["Media"][0]["Part"][0]
        self.assertEqual(mde_part["decision"], "transcode")
        mde_audio = next(s for s in mde_part["Stream"] if s["streamType"] == 2)
        self.assertEqual((mde_audio["decision"], mde_audio["codec"]), ("transcode", "ac3"))

        pms.refuse_enhancements = True
        refused = get(pms, path + "&normalizeLoudness=1")
        self.assertEqual(refused["generalDecisionCode"], 2000)
        self.assertEqual(refused["Metadata"], [])
        self.assertTrue(refused["transcodeDecisionText"])
        pms.refuse_enhancements = False

        pms.ignore_enhancements = True
        ignored = get(pms, path + "&normalizeLoudness=1")
        ig_part = ignored["Metadata"][0]["Media"][0]["Part"][0]
        self.assertEqual(ig_part["decision"], "transcode")
        ig_audio = next(s for s in ig_part["Stream"] if s["streamType"] == 2)
        self.assertEqual(ig_audio["decision"], "copy")

    @staticmethod
    def _requests(pms):
        status, ctype, body = pms.handle("GET", "/_mock/requests")
        assert status == 200 and ctype == "application/json", (status, ctype)
        return json.loads(body)

    def test_mock_config_runtime_toggle(self):
        pms = MockPms(Library())
        status, _, body = pms.handle(
            "POST", "/_mock/config", json.dumps({"refuse_enhancements": True}).encode())
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body), {
            "refuse_enhancements": True, "ignore_enhancements": False, "plex_pass": True})
        self.assertIs(pms.refuse_enhancements, True)
        self.assertIs(pms.ignore_enhancements, False)

        status, _, body = pms.handle("POST", "/_mock/config", json.dumps({"bogus": True}).encode())
        self.assertEqual(status, 400)

        # a case can resolve once, then refuse only from the next decision on
        pms2 = MockPms(Library())
        rk = ENHANCEMENT_AC3_2CH_RK
        path = (f"/video/:/transcode/universal/decision?path=%2Flibrary%2Fmetadata%2F{rk}"
                "&directPlay=0&directStream=1&normalizeLoudness=1")
        self.assertEqual(get(pms2, path)["generalDecisionCode"], 1000)
        pms2.handle("POST", "/_mock/config", json.dumps({"refuse_enhancements": True}).encode())
        self.assertEqual(get(pms2, path)["generalDecisionCode"], 2000)

    def test_request_log(self):
        pms = MockPms(Library())
        pms.handle("GET", "/identity?X-Plex-Token=SECRET&foo=bar",
                  headers={"X-Plex-Session-Identifier": "sess-1"})
        log = self._requests(pms)
        entry = next(e for e in log if e["path"] == "/identity")
        self.assertEqual(entry["method"], "GET")
        self.assertEqual(entry["query"], {"foo": "bar"})
        self.assertNotIn("X-Plex-Token", entry["query"])
        self.assertEqual(entry["session"], "sess-1")

        status, _, _ = pms.handle("DELETE", "/_mock/requests")
        self.assertEqual(status, 200)
        after = self._requests(pms)
        self.assertEqual(len(after), 1)
        self.assertEqual((after[0]["method"], after[0]["path"]), ("GET", "/_mock/requests"))

    @unittest.skipUnless(
        HAS_FFMPEG_AND_FFPROBE,
        "needs ffmpeg+ffprobe; the host CI runner has neither",
    )
    def test_transcode_fixture_served(self):
        with tempfile.TemporaryDirectory() as tmp:
            clip = pathlib.Path(tmp) / "fixture.mkv"
            subprocess.check_call([
                "ffmpeg", "-y", "-v", "error", "-f", "lavfi", "-i",
                "color=size=64x64:rate=24:duration=1", "-f", "lavfi", "-i",
                "sine=frequency=330:duration=1", "-c:v", "libx264", "-c:a", "aac", "-t", "1",
                str(clip)])
            server, pms = serve(0, movies=0, transcode_fixture=clip)
            try:
                base = f"http://127.0.0.1:{server.server_address[1]}"
                url = base + "/video/:/transcode/universal/start.mkv?path=x"
                with urllib.request.urlopen(url, timeout=5) as response:
                    self.assertEqual(response.status, 200)
                    self.assertEqual(response.read(), clip.read_bytes())
                    self.assertEqual(response.headers["Accept-Ranges"], "bytes")
                request = urllib.request.Request(url, headers={"Range": "bytes=1-3"})
                with urllib.request.urlopen(request) as response:
                    self.assertEqual(response.status, 206)
                    self.assertEqual(response.read(), clip.read_bytes()[1:4])
                    self.assertTrue(response.headers["Content-Range"].startswith("bytes 1-3/"))
            finally:
                server.shutdown()
                server.server_close()

    def test_missing_transcode_fixture_file_is_rejected(self):
        with self.assertRaises(ValueError):
            serve(0, movies=0, transcode_fixture=pathlib.Path("/no/such/fixture.mkv"))


# The fields the detail page draws that the demo catalog can supply (docs: the S1a stage of the
# demo-library plan). Every one is asserted against what the MOCK serves, because a field the mock
# omits is a field the screenshots and the site video silently never show.
@unittest.skipUnless(HAS_DEMO_CACHE, "no derived demo cache (make demo-library) or no ffmpeg/ffprobe")
class CatalogDetailFields(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.lib = CatalogLibrary(CATALOG)
        cls.pms = MockPms(cls.lib)
        cls.catalog = json.loads(CATALOG.read_text())

    def detail(self, rk, pms=None):
        return get(pms or self.pms, f"/library/metadata/{rk}")["Metadata"][0]

    def rk_of(self, film):
        return int(self.lib.by_slug[film])

    def test_a_complete_title_serves_the_fields_the_detail_page_draws(self):
        for film in ("sintel", "tears-of-steel"):
            with self.subTest(film=film):
                rec = next(m for m in self.catalog["movies"] if m["id"] == film)
                d = self.detail(self.rk_of(film))
                self.assertEqual(d["tagline"], rec["tagline"])
                self.assertEqual([c["tag"] for c in d["Country"]], rec["countries"])
                self.assertEqual([w["tag"] for w in d["Writer"]], rec["writers"])
                self.assertEqual([r["tag"] for r in d["Role"]], [c["name"] for c in rec["cast"]])
                self.assertEqual([r.get("role", "") for r in d["Role"]],
                                 [c.get("role", "") for c in rec["cast"]])
                self.assertEqual([x["tag"] for x in d["Director"]], rec["directors"])

    def test_an_uncited_rating_is_never_served(self):
        # Wikidata holds no review score and no certification for these two, so the mock serves
        # none: the app then shows "Unrated" and no ratings row, not an invented number.
        for film in ("sintel", "tears-of-steel"):
            with self.subTest(film=film):
                d = self.detail(self.rk_of(film))
                self.assertNotIn("contentRating", d)
                self.assertEqual(d.get("Rating", []), [])
                for flat in ("rating", "audienceRating", "ratingImage", "audienceRatingImage"):
                    self.assertNotIn(flat, d)

    def test_the_related_hub_is_the_whole_list_so_no_row_waits_for_a_page(self):
        hubs = get(self.pms, f"/library/metadata/{self.rk_of('tears-of-steel')}/related")["Hub"]
        related = next(h for h in hubs if h["hubIdentifier"] == "related")
        self.assertFalse(related["more"])
        self.assertEqual(related["size"], related["totalSize"])
        self.assertEqual(len(related["Metadata"]), len(self.catalog["movies"]) - 1)
        self.assertTrue(all(m.get("thumb") for m in related["Metadata"]), "every card has a poster")

    def test_a_person_without_a_catalog_photo_is_served_without_one(self):
        # Only the catalog's `people` have a portrait; for everyone else the app draws its glyph.
        for film, jobs in (("sintel", ("Role", "Director", "Writer")), ("tears-of-steel", ("Director", "Writer"))):
            d = self.detail(self.rk_of(film))
            for job in jobs:
                for person in d[job]:
                    self.assertNotIn("thumb", person, (film, job, person))

    def test_a_person_with_a_catalog_photo_is_served_it_through_the_transcoder(self):
        d = self.detail(self.rk_of("tears-of-steel"))
        people = {p["tag"]: p for p in d["Role"]}
        self.assertEqual({name for name, p in people.items() if "thumb" in p}, set(self.catalog["people"]))
        self.assertEqual(len(people), 7, "the whole cast")
        seen = set()
        for name in self.catalog["people"]:
            ctype, body = self.lib.image(people[name]["thumb"], 300, 300)
            self.assertEqual(ctype, "image/jpeg")
            self.assertEqual(body[:2], b"\xff\xd8", name)
            seen.add(body)
        self.assertEqual(len(seen), len(self.catalog["people"]), "each person has their own picture")

    def _temp_library(self, edit):
        """A CatalogLibrary over a copy of the catalog that `edit` has changed."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        cat = json.loads(CATALOG.read_text())
        edit(cat)
        (root / "catalog.json").write_text(json.dumps(cat))
        shutil.copy(CATALOG.parent / "assets.json", root / "assets.json")
        lib = CatalogLibrary(root / "catalog.json")
        return lib, MockPms(lib)

    def test_a_cited_content_rating_and_ratings_are_served_as_a_server_sends_them(self):
        cited = {"source": "https://example.invalid/cite", "retrieved": "2026-10-07"}

        def edit(cat):
            m = next(m for m in cat["movies"] if m["id"] == "sintel")
            m["contentRating"] = dict(cited, value="PG")
            m["ratings"] = [dict(cited, image="rottentomatoes://image.rating.ripe", value=9.1, type="critic"),
                            dict(cited, image="imdb://image.rating", value=7.5, type="audience")]

        lib, pms = self._temp_library(edit)
        d = self.detail(int(lib.by_slug["sintel"]), pms)
        self.assertEqual(d["contentRating"], "PG")
        self.assertEqual(d["Rating"], [
            {"image": "rottentomatoes://image.rating.ripe", "value": 9.1, "type": "critic"},
            {"image": "imdb://image.rating", "value": 7.5, "type": "audience"}])
        self.assertEqual((d["rating"], d["ratingImage"]), (9.1, "rottentomatoes://image.rating.ripe"))
        self.assertEqual((d["audienceRating"], d["audienceRatingImage"]), (7.5, "imdb://image.rating"))

    def test_show_creators_are_writers_and_episodes_inherit_the_shows_credits(self):
        def edit(cat):
            show = next(s for s in cat["shows"] if s["id"] == "caminandes")
            show["creators"] = ["Pablo Vazquez"]
            show["cast"] = [{"name": "A Voice", "role": "Koro"}]
            show["countries"] = ["Netherlands"]

        lib, pms = self._temp_library(edit)
        show = self.detail(int(lib.by_slug["caminandes"]), pms)
        self.assertEqual([w["tag"] for w in show["Writer"]], ["Pablo Vazquez"])
        self.assertEqual([c["tag"] for c in show["Country"]], ["Netherlands"])
        ep = self.detail(int(lib.by_slug["caminandes/1/1"]), pms)
        for job in ("Director", "Writer", "Role"):
            self.assertEqual(ep[job], show[job], job)
        self.assertEqual([c["tag"] for c in ep["Role"]], ["A Voice"])


class DetailFixtures(unittest.TestCase):
    """`tests/demo_library/fixtures/detail-<rk>.json` is the mock's `/library/metadata/<rk>`
    answer for each fixture title, committed so the data crate can parse it
    (`rust-modules/data/src/metadata_demo_fixture_tests.rs`) without a derived-art cache."""
    TITLES = {"sintel": 102, "tears-of-steel": 105}

    def test_each_fixture_carries_every_field_the_catalog_supplies(self):
        cat = json.loads(CATALOG.read_text())
        for film, rk in self.TITLES.items():
            with self.subTest(film=film):
                rec = next(m for m in cat["movies"] if m["id"] == film)
                d = json.loads((FIXTURES / f"detail-{rk}.json").read_text())["MediaContainer"]["Metadata"][0]
                self.assertEqual(d["title"], rec["title"])
                self.assertEqual(d["tagline"], rec["tagline"])
                self.assertEqual([c["tag"] for c in d["Country"]], rec["countries"])
                self.assertEqual([w["tag"] for w in d["Writer"]], rec["writers"])
                self.assertEqual([x["tag"] for x in d["Director"]], rec["directors"])
                self.assertEqual([r["tag"] for r in d["Role"]], [c["name"] for c in rec["cast"]])

    @unittest.skipUnless(HAS_DEMO_CACHE, "no derived demo cache (make demo-library) or no ffmpeg/ffprobe")
    def test_each_fixture_is_what_the_mock_serves_now(self):
        pms = MockPms(CatalogLibrary(CATALOG))
        for film, rk in self.TITLES.items():
            with self.subTest(film=film):
                status, _, body = pms.handle("GET", f"/library/metadata/{rk}")
                self.assertEqual(status, 200)
                self.assertEqual(json.loads(body), json.loads((FIXTURES / f"detail-{rk}.json").read_text()),
                                 "regenerate with `python3 tools/demo_library.py fixtures`")


def first_of(lib, kind):
    return next(it for it in lib.items.values() if it["type"] == kind)


def window(path, start, size, headers=False):
    """`path` paged by the container window, as a query parameter or, with `headers`, as headers."""
    if headers:
        return path, {"X-Plex-Container-Start": str(start), "X-Plex-Container-Size": str(size)}
    sep = "&" if "?" in path else "?"
    return f"{path}{sep}X-Plex-Container-Start={start}&X-Plex-Container-Size={size}", None


def fetch(pms, path, headers=None):
    status, ctype, data = pms.handle("GET", path, headers=headers)
    assert (status, ctype) == (200, "application/json"), (status, ctype, path)
    return json.loads(data)["MediaContainer"]


class ContainerPaging(unittest.TestCase):
    """The PMS paging contract (docs/pms-api.md, "Paging, observed"): a request pages only when it
    carries the container window, as a query parameter or as a header; a paged answer echoes
    `offset` and carries the listing's `totalSize`; an unpaged answer carries neither."""

    def assertPaged(self, container, start, rows, total):
        self.assertEqual(container["totalSize"], total)
        self.assertEqual(container["offset"], start)
        self.assertEqual(container["size"], len(rows))

    def test_unpaged_answers_carry_no_total_and_no_offset(self):
        lib = Library()
        pms = MockPms(lib)
        show = first_of(lib, "show")
        season = next(it for it in lib.items.values() if it["type"] == "season")
        title = first_of(lib, "movie")["title"]
        for path in ("/library/sections/1/all", f"/library/metadata/{show['ratingKey']}/children",
                     f"/library/metadata/{season['ratingKey']}/children",
                     f"/library/metadata/{first_of(lib, 'movie')['ratingKey']}/similar",
                     "/hubs/continueWatching/items", "/hubs", "/hubs/sections/1",
                     f"/library/search?query={title}&searchTypes=movies"):
            with self.subTest(path=path):
                container = fetch(pms, path)
                self.assertNotIn("totalSize", container)
                self.assertNotIn("offset", container)

    def test_a_size_without_a_start_is_ignored(self):
        """docs/pms-api.md, "Paging gotcha": a query-param Size without Start changes nothing."""
        pms = MockPms(Library())
        ignored = fetch(pms, "/library/sections/1/all?X-Plex-Container-Size=5")
        self.assertEqual(len(ignored["Metadata"]), 48)
        self.assertNotIn("totalSize", ignored)
        self.assertNotIn("offset", ignored)

    def test_window_as_query_and_as_header_answer_alike(self):
        pms = MockPms(Library())
        by_query = fetch(pms, window("/library/sections/1/all", 10, 5)[0])
        by_header = fetch(pms, *window("/library/sections/1/all", 10, 5, headers=True))
        self.assertEqual(by_header, by_query)
        self.assertPaged(by_query, 10, by_query["Metadata"], 48)
        self.assertEqual(len(by_query["Metadata"]), 5)

    def test_size_zero_is_a_paged_request_that_reads_the_total(self):
        pms = MockPms(Library())
        page = fetch(pms, window("/library/sections/1/all", 0, 0)[0])
        self.assertEqual(page["Metadata"], [])
        self.assertPaged(page, 0, [], 48)

    def test_start_past_the_end_is_empty_and_still_echoes_the_offset(self):
        pms = MockPms(Library())
        page = fetch(pms, window("/library/sections/1/all", 500, 10)[0])
        self.assertEqual(page["Metadata"], [])
        self.assertPaged(page, 500, [], 48)

    def test_children_of_a_show_and_of_a_season_page(self):
        lib = Library()
        pms = MockPms(lib)
        show = first_of(lib, "show")["ratingKey"]
        season = next(it["ratingKey"] for it in lib.items.values() if it["type"] == "season")
        for parent in (show, season):
            with self.subTest(parent=parent):
                full = fetch(pms, f"/library/metadata/{parent}/children")["Metadata"]
                self.assertGreaterEqual(len(full), 2)
                page = fetch(pms, window(f"/library/metadata/{parent}/children", 1, 1)[0])
                self.assertEqual(page["Metadata"], full[1:2])
                self.assertPaged(page, 1, full[1:2], len(full))

    def test_extras_page_and_default_library_has_none(self):
        lib = Library(extras=3)
        pms = MockPms(lib)
        movie = first_of(lib, "movie")["ratingKey"]
        full = fetch(pms, f"/library/metadata/{movie}/extras")["Metadata"]
        self.assertEqual(len(full), 3)
        self.assertEqual({row["type"] for row in full}, {"clip"})
        self.assertEqual(len({row["ratingKey"] for row in full}), 3)
        self.assertNotIn("totalSize", fetch(pms, f"/library/metadata/{movie}/extras"))
        page = fetch(pms, window(f"/library/metadata/{movie}/extras", 1, 2)[0])
        self.assertEqual(page["Metadata"], full[1:3])
        self.assertPaged(page, 1, full[1:3], 3)
        default = MockPms(Library())
        self.assertEqual(fetch(default, f"/library/metadata/{movie}/extras")["Metadata"], [])

    def test_person_media_pages(self):
        lib = Library()
        pms = MockPms(lib)
        pid = max(lib.people, key=lambda p: len(lib.person_media(p)))
        self.assertGreaterEqual(len(lib.person_media(pid)), 2, "the fixture gives a person two credits")
        full = fetch(pms, f"/library/people/{pid}/media")["Metadata"]
        self.assertEqual(full, lib.person_media(pid))
        page = fetch(pms, window(f"/library/people/{pid}/media", 1, 1)[0])
        self.assertEqual(page["Metadata"], full[1:2])
        self.assertPaged(page, 1, full[1:2], len(full))

    def test_similar_pages_over_the_same_type_without_the_item_itself(self):
        lib = Library()
        pms = MockPms(lib)
        movie = first_of(lib, "movie")
        full = fetch(pms, f"/library/metadata/{movie['ratingKey']}/similar")["Metadata"]
        self.assertEqual(len(full), sum(1 for it in lib.items.values() if it["type"] == "movie") - 1)
        self.assertNotIn(movie["ratingKey"], {row["ratingKey"] for row in full})
        self.assertEqual({row["type"] for row in full}, {"movie"})
        page = fetch(pms, window(f"/library/metadata/{movie['ratingKey']}/similar", 2, 3)[0])
        self.assertEqual(page["Metadata"], full[2:5])
        self.assertPaged(page, 2, full[2:5], len(full))

    def test_continue_watching_items_page_contiguously(self):
        lib = Library()
        pms = MockPms(lib)
        # The default library already has in-progress items; the test sets its own five.
        for it in lib.items.values():
            it["viewOffset"] = 0
        for n, movie in enumerate([it for it in lib.items.values() if it["type"] == "movie"][:5]):
            movie["viewOffset"] = 1000
            movie["lastViewedAt"] = 1_700_000_000 + n
        full = lib.continue_watching()
        self.assertEqual(len(full), 5)
        first = fetch(pms, window("/hubs/continueWatching/items", 0, 3)[0])
        second = fetch(pms, window("/hubs/continueWatching/items", 3, 3)[0])
        self.assertPaged(first, 0, first["Metadata"], 5)
        self.assertPaged(second, 3, second["Metadata"], 5)
        self.assertEqual(first["Metadata"] + second["Metadata"], full)

    def test_hubs_page_by_hub_on_both_hub_listings(self):
        lib = Library()
        pms = MockPms(lib)
        pms.home_hubs = 6
        full = fetch(pms, "/hubs")["Hub"]
        self.assertEqual(len(full), 6)
        page = fetch(pms, window("/hubs", 2, 3)[0])
        self.assertEqual(page["Hub"], full[2:5])
        self.assertPaged(page, 2, full[2:5], 6)
        header_page = fetch(pms, *window("/hubs", 2, 3, headers=True))
        self.assertEqual(header_page, page)
        section = fetch(pms, "/hubs/sections/1")["Hub"]
        self.assertEqual(len(section), 2)
        section_page = fetch(pms, window("/hubs/sections/1", 1, 1)[0])
        self.assertEqual(section_page["Hub"], section[1:2])
        self.assertPaged(section_page, 1, section[1:2], 2)

    def test_library_search_rows_page_by_offset_and_limit_caps_the_total(self):
        lib = Library()
        pms = MockPms(lib)
        prefix = first_of(lib, "movie")["title"][:2]
        hits = [it for it in lib.items.values() if it["type"] == "movie" and it["title"].startswith(prefix)]
        self.assertGreaterEqual(len(hits), 2, "the prefix matches at least two movies")
        path = f"/library/search?query={prefix}&searchTypes=movies"
        full = fetch(pms, path)["SearchResult"]
        self.assertEqual(len(full), len(hits))
        for row in full:
            self.assertEqual(set(row), {"Metadata", "score"})
            self.assertTrue(row["Metadata"]["title"].startswith(prefix))
        page = fetch(pms, window(path, 1, 1)[0])
        self.assertEqual(page["SearchResult"], full[1:2])
        self.assertPaged(page, 1, full[1:2], len(full))
        capped = fetch(pms, window(f"{path}&limit=1", 0, 5)[0])
        self.assertEqual(len(capped["SearchResult"]), 1)
        self.assertPaged(capped, 0, capped["SearchResult"], 1)

    def test_library_search_types_order_and_unknown_kinds(self):
        lib = Library()
        pms = MockPms(lib)
        # Known titles under one prefix, so the order each type lists in is what is asserted.
        movie = first_of(lib, "movie")
        show = first_of(lib, "show")
        episode = next(it for it in lib.items.values() if it["type"] == "episode")
        movie["title"], show["title"], episode["title"] = "s0000abc0", "s0000abc1", "s0000abc2"
        person = lib.people[1]
        person["tag"] = "s0000abc9"
        query = "s0000abc"

        def titles(kinds):
            rows = fetch(pms, f"/library/search?query={query}&searchTypes={kinds}")["SearchResult"]
            return [row["Metadata"]["title"] for row in rows]

        self.assertEqual(titles("tv"), ["s0000abc1", "s0000abc2"], "shows first, then episodes")
        self.assertEqual(titles("movies,tv"), ["s0000abc0", "s0000abc1", "s0000abc2"],
                         "one mixed list in movies, shows, episodes order")
        people = fetch(pms, f"/library/search?query={query}&searchTypes=people")["SearchResult"]
        self.assertEqual(len(people), 1)
        self.assertEqual(people[0]["Directory"]["tagKey"], person["tagKey"])
        self.assertEqual(people[0]["Directory"]["count"], len(lib.person_media(person["id"])))
        self.assertIn("score", people[0])
        for kinds in ("shows", "bogus", "music"):
            with self.subTest(searchTypes=kinds):
                empty = fetch(pms, window(f"/library/search?query={query}&searchTypes={kinds}", 0, 10)[0])
                self.assertEqual(empty["SearchResult"], [])
                self.assertPaged(empty, 0, [], 0)


class BifStills(unittest.TestCase):
    """`/library/parts/<part>/indexes/sd/<ms>` through the photo transcoder: the scrub preview's
    stills."""

    @unittest.skipUnless(HAS_FFMPEG_AND_FFPROBE, "no ffmpeg/ffprobe")
    def test_a_part_with_a_file_answers_a_jpeg_in_the_films_aspect_and_one_without_404s(self):
        with tempfile.TemporaryDirectory() as tmp:
            film = pathlib.Path(tmp) / "film.mkv"
            subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i",
                            "testsrc=size=480x200:rate=5:duration=25", "-pix_fmt", "yuv420p", str(film)],
                           check=True)
            lib = object.__new__(CatalogLibrary)
            lib.__dict__.update(Library(movies=3).__dict__)
            lib._scaled, lib._lock = {}, __import__("threading").Lock()
            lib.media_files = {7459: film}
            url = "/library/parts/7459/indexes/sd/14000"
            ctype, data = lib.image(url, "320", "180")
            self.assertEqual((ctype, data[:2]), ("image/jpeg", b"\xff\xd8"))
            probe = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=width,height",
                                    "-of", "csv=p=0", "-"], input=data, capture_output=True)
            self.assertEqual(probe.stdout.decode().strip(), "320,134", "320 wide, the film's 2.4:1 aspect")
            # the nearest 10 s still: 14 s and 10 s are the same picture
            self.assertEqual(lib.image("/library/parts/7459/indexes/sd/10000", "320", "180")[1], data)
            self.assertIsNone(lib.image("/library/parts/7460/indexes/sd/0", "320", "180"), "no file: 404")
            self.assertIsNone(lib.image("/library/parts/7459/indexes/sd/9999999", "320", "180"),
                              "past the end: 404")

    def test_without_any_media_file_there_is_no_index(self):
        lib = object.__new__(CatalogLibrary)
        lib.__dict__.update(Library(movies=3).__dict__)
        lib._scaled, lib._lock = {}, __import__("threading").Lock()
        lib.media_files = {}
        self.assertIsNone(lib.image("/library/parts/7459/indexes/sd/0", "320", "180"))


class PmsAudioVocabulary(unittest.TestCase):
    """The mock speaks PMS's codec names, not FFmpeg's: DTS is `dca` on the wire."""

    def test_dts_is_dca_on_the_wire_and_other_codecs_pass_through(self):
        from mock_pms import pms_audio_codec
        self.assertEqual(pms_audio_codec("dts"), "dca")
        self.assertEqual([pms_audio_codec(c) for c in ("ac3", "eac3", "aac", "truehd")],
                         ["ac3", "eac3", "aac", "truehd"])
        stream = Library(movies=1)._stream_wire({"codec_type": "audio", "codec_name": "dts", "index": 1}, 11)
        self.assertEqual(stream["codec"], "dca")
        media = Library(movies=1)._media(__import__("random").Random(1), 5, 50, 1000, acodec="dts")[0]
        self.assertEqual(media["audioCodec"], "dca")
        self.assertEqual(media["Part"][0]["Stream"][1]["codec"], "dca")


if __name__ == "__main__":
    unittest.main()
