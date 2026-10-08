# Search fixtures (spec 0007, AC2-AC3)

Written from the live probe of 2026-10-08 (spec 0007 "Facts vs.
assumptions"): catalogue items as Tidal returned them, trimmed to a few per
list and to the recorded keys; `userId` replaced.

- `search_page.json`: `GET /search?types=TRACKS,ALBUMS,ARTISTS,PLAYLISTS`
  for "pierce the veil": four lists plus an empty `videos`, an `ARTISTS` top
  hit; *Tracks* holds a Dolby-Atmos-only copy (`LOW`, `["DOLBY_ATMOS"]`) of
  "Hell Above" (recorded for "hell above"); playlists carry `creator: {}`
- `search_empty.json`: a query with no match: every list empty, `topHit: null`
- `search_tracks.json`, `search_albums.json`, `search_artists.json`,
  `search_playlists.json`: `GET /search/{type}` pages, bare items; tracks and
  albums add `artist`, tracks leave out `album.releaseDate`, playlists carry
  `creator: {id: 0}`
- `search_tracks_past_end.json`: `offset=300` past a total of 295: `200`, no
  items

The other top-hit kinds are built by the tests from these items: `TRACKS`
and `ALBUMS` were recorded (value = the full item); **`PLAYLISTS` and any
other `type` are assumed**, never recorded (spec 0007 "Not verified").
