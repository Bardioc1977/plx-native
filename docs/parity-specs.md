# Plex server parity specs

One spec per gap between PlxNative and what a current Plex Media Server offers for movies and TV.
The candidate list comes from a code audit on 2026-10-24 against PMS 1.43.5.11029;
`docs/parity-gaps.md` is the older (2026-07-29) inventory and is partly closed.

Priority is by what the owner's library actually exercises (multi-version files, video playlists,
28 server-side filters per library), then by reach.

| # | gap | spec | status |
|---|---|---|---|
| 1 | Version selection for multi-version media | `docs/version-selection-spec.md` | proposed |
| 2 | Video playlists: browse, play, shuffle | `docs/playlists-spec.md` | proposed |
| 3 | Library filter facets from the server's `Meta.Type.Filter[]` | not written | |
| 4 | Companion: be a cast/remote-control target (`/player/*`, GDM) | not written | |
| 5 | User ratings: read and write (`/:/rate`), reviews | not written | |

Not planned here: Plex Discover browse, friends/social, Live TV, music, photos (out of scope by
earlier decision), and accessibility/voice search (little use on this device).
