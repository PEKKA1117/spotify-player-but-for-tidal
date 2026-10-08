# Library fixtures (spec 0006, AC4-AC7)

Written from the shapes recorded by the live probes (spec 0006 "Facts vs.
assumptions", 2026-10-07). IDs, titles, names and UUIDs are fakes; keys and
value types are the recorded ones. Tracks are copies of
`../metadata/track.json` (the tests also build longer lists from it).

- `playlists.json`: `playlistsAndFavoritePlaylists` entries, one
  `USER_CREATED`, one `USER_FAVORITE`
- `favorite_albums.json`, `favorite_artists.json`: `{created, item}` entries
- `album.json`, `playlist.json`, `artist.json`: the page headers
- `albums_plain.json`, `albums_eps_and_singles.json`,
  `albums_compilations.json`, `albums_empty.json`: `/artists/{id}/albums`
  with no filter, `EPSANDSINGLES`, `COMPILATIONS`, and an unknown artist's `200`
- `playlist_items_with_video.json`: `/playlists/{uuid}/items` with a video
- `contributor_page.json`, `contributor_page_no_module.json`,
  `contributor_data_page.json`: `/pages/contributor` and a `dataApiPath` page
  (an Atmos-only track, an `Instrumental` one, an unknown role category)
- `error_*.json`: the recorded error bodies (`2001`, `7002`, `1001`)
- `created_playlist.json`, `add_items_ok.json`: the write probe's answers

**Assumed, not recorded** (spec 0006 "Not verified"):

- `favorites_ids.json`: `GET /users/{user}/favorites/ids` was never probed;
  the shape `{"TRACK": [...], "ALBUM": [...], "ARTIST": [...], "PLAYLIST":
  [...]}` with ID strings is a guess
- the `type` key naming the contributor page's module
  (`"type": "ITEM_LIST_WITH_ROLES"`) and `creator.id` on a playlist
- a `200` with no items (and the total) for `offset` beyond the end
