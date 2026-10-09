# 0006 — Library: favorites, playlists, album and artist pages

- **Status**: implemented (2026-10-07; the manual checks under "Test plan" are run on the user's machine)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0002 (implemented: the session's `user_id` and `country_code`), 0004 (implemented: the queue, `LoadQueue`/`AddToQueue`, the TUI), 0005 (implemented: the socket, the client/player split, `Open`)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Pages", "The library", "Album, playlist and artist pages" and "Actions" sections and the new keys; [`docs/playback.md`](../playback.md) "Settings" gains two settings (AC20)

## Context

Until now the queue can only be filled from pasted links and IDs (0004 decision 1). This spec adds what spotify-player calls **pages**: the user's library (their playlists, favorite albums and favorite artists), their favorite tracks, and album, playlist and artist pages reached from them, with spotify-player's navigation (a page history, focusable windows, an actions popup). From any track list a track plays **with the rest of its list** queued after it, or is added to the queue; tracks, albums, artists and playlists can be added to or removed from favorites, and the user's own playlists created, added to, trimmed and deleted. It also gives the queue its long-promised remove key (0004 "Out of scope").

It settles 0005's open question (0005 decision 3, "Out of scope"): **the player fetches the pages**, as it already expands `Open`. A client still holds no session, no keyring entry and no API access (0005 AC14 keeps holding).

Search (0007), mixes and radio as pages (0011) are not in this spec; 0009 decided against caching.

### What tidalt did

Read from tidalt's `internal/tidal/{api,library}.go`, `internal/ui/{keys,model}.go`, `docs/layout.md` and the commits named:

- **One page per list, no paging**: favorite tracks were fetched with `limit=50` (`GetFavorites(ctx, 50)`), favorite artists and albums with `limit=200`, the user's playlists with `limit=50`, all as a single request with `order=DATE&orderDirection=DESC`; album tracks with no `limit` at all. A user with more favorites saw only the first 50 (or 200), with nothing saying so. Playlist tracks asked for `limit=1000`, which `/items` refuses with a `400` (0004 "Facts"; whether `/tracks`, tidalt's endpoint, refuses it too is not verified). Only artist albums were paged (50 per page, at most 20 pages)
- **Only the user's own playlists**: `/users/{id}/playlists`; playlists the user had added to their favorites were not listed
- Playlists were loaded from `/v1/playlists/{uuid}/tracks`, which returns videos as if they were tracks (0004 "What went wrong" 8)
- **Favorite Songs showed the queue** instead of the favorites (`65285f1`): the favorites were kept only as an ID set for the ♥ marker, and the page drew the playback queue
- **Counts read 0 until a section was opened** (`64d3695`), fixed by prefetching everything at start; revisits then never refreshed
- **An album in the artist view went straight into the queue** (`3b6d6ea`) with no way to see its tracks first
- Playing from a list queued only the chosen track, so the queue ended after it (0004 "What went wrong" 9, `59c610d`)
- A client playing a playlist sent `PlayTrackID` and the player's queue shrank to one track (0005 "What went wrong" 2, #5)
- A playlist sent from a shuffled client arrived in display order and was stored as the original order (0005 "What went wrong" 3, #21)
- Its later Library page (`90b5130`, spotify-player style) put Playlists, Artists and Albums side by side at 40/20/40 %, one focused column cycled with `Tab`/`Shift-Tab`, only the focused column below 60 columns, `Backspace` back: kept here, in spotify-player's window order

### What could go wrong here, and the criterion that covers it

1. **Playing from a list plays one track** (tidalt `59c610d`, #5) → `Enter` on a track row sends the **whole list** (loading the rest first) with the chosen index, as one `LoadQueue` (AC12)
2. **Lists sent in a display order** (tidalt #21) → a page's list is sent in the order Tidal returned it; the client never sorts or shuffles it (AC12)
3. **Videos queued as tracks** → favorites, playlist and artist lists keep only tracks; video items are dropped where the API mixes them in (AC4)
4. **A list cut at one page** (tidalt: 50 favorite tracks, 50 playlists, 200 albums and artists; Tidal's default page is 10 items on several endpoints, 0004 "Facts") → every list loads to its end as you scroll, and the title shows Tidal's total from the first page (AC4, AC10)
5. **Followed playlists missing** (tidalt listed only its own) → the library lists own and favorite playlists (AC4)
6. **A page that draws another list** (tidalt `65285f1`: Favorite Songs drew the queue) → each page renders its own fetched data only (AC17, a snapshot per page)
7. **Stale or zero counts** (tidalt `64d3695`) → counts come with each fetch, and opening a page always fetches it fresh (AC9)
8. **Browsing that stalls playback**: a fetch of thousands of favorites must never delay `Next` → fetches run beside the player's input loop, never in it (AC8)
9. **A result landing on the wrong page**: the user moved on while a fetch was in flight → every fetch carries an ID, and a result is applied only to the page that asked for it (AC9)
10. **A client that needs its own login**, as tidalt's did (0005 "What went wrong" 9) → pages come through the player (AC8, AC19)
11. **An album opened from an artist plays at once** (tidalt `3b6d6ea`) → `Enter` on an album opens its page; only `Enter` on a track plays (AC12)
12. **A short page taken for overlap or the end** (the probe: 358 favorite tracks on a 1000-item page for a total of 362) → `offset` advances by the page size asked, rows already loaded are skipped (AC4, AC10)
13. **A playlist edit by a stale position** (positions shift after each removal; Tidal checks the ETag) → removals send the ETag the list was loaded with and are never retried on `412` (AC7, AC14)
14. **Credits padded with alternate and Atmos-only copies** (the contributor probe: Dolby-Atmos-only tracks; the user: instrumental, TV size, sped up, slowed + reverb) → *All tracks* hides them by a configurable word list and says how many (AC3, AC6)

## Behaviour

### Pages

The area below the playback window shows one **page** at a time. The queue (0004) is the bottom page of the history; the TUI **starts on the library** (the user, 2026-10-08), pushed on top of the queue, so `Backspace` goes to the queue. The library shows `Loading…` until the player's first `Welcome` arrives, then is fetched (a client asks nothing before it is connected). Opening a page puts it on top of a **history**; going back returns to the page under it, with its loaded rows, cursors and focus as they were, without fetching again. Opening a page always fetches it fresh (nothing is cached; 0009 decision 3).

| Page | Opened with | Windows (`Tab` moves between them) |
|---|---|---|
| Queue | `z`; `Backspace` from the start page | the queue (0004) |
| Library | `g l`; the page at start | **Playlists**, **Albums**, **Artists** |
| Favorite tracks | `g y` | the tracks |
| Album | `Enter` on an album; *Go to album* | the album's tracks |
| Playlist | `Enter` on a playlist | the playlist's tracks |
| Artist | `Enter` on an artist; *Go to artist* | **Top tracks**, **Albums** (albums, then EPs and singles), **Appears on**, **All tracks** |

- Opening the page that is already on top does nothing. Otherwise the new page is pushed; the history keeps at most **50** pages (the oldest is dropped; the queue at the bottom is never dropped)
- `Backspace` (and `C-q`) goes back; on the bottom page it does nothing
- `z`, `g l`, `g y` open their page from anywhere (pushed like any other)
- Each page has a **title row**; counts are Tidal's totals (`totalNumberOfItems`), known from the first page:
  - Library: `Library`
  - Favorite tracks: `Favorite tracks · 362 tracks`
  - Album: `<title> · <artists> · <year> · 17 tracks · 1:02:15` (year and length when known)
  - Playlist: `<title> · 39 tracks · 2:41:07`
  - Artist: `<name>`
- A page being fetched shows `Loading…` in its windows; a failed fetch shows the message in the page (`Could not load the library: <reason>`, `Album 1 was not found`, …); an empty list shows `No favorite tracks yet`, `No playlists yet`, `No favorite albums yet`, `No favorite artists yet`, `This playlist has no tracks`, `No albums`, `No top tracks`, `No credits`

### Lists load as you scroll

Every list (each window above) is fetched a **page** at a time (decision 5): the first page with the page itself, then the next when the window's cursor comes within one window height of the last loaded row. While it loads, the last row says `Loading more…`; a failed page leaves the loaded rows and shows its message as the last row, and the next cursor move near the end tries once more. Tidal's total is shown at once; rows beyond the loaded ones are not drawn.

- **Page size**: a setting, `TIDAL_PLAYER_PAGE_SIZE` (until 0008 moves it to `app.toml`), integer **1–10 000**, default **100**; empty = unset, invalid → exit 2 naming the variable (0004 "Settings" rules). Each request asks `min(page size, endpoint's largest page)` (table under "What the player fetches": 50 for playlists and credits)
- **The whole list is needed** for `Enter` on a track (play from here), `Z` on a list, *Add to playlist* of a list: the client first fetches the remaining pages (`Loading 300 of 1 234…` in the message row), then sends one command with every track in page order. `Esc` cancels; nothing is sent. A list above **40 000** tracks is refused with `Too many tracks to queue at once (N)` (the socket's 16 MiB line holds about 50 000)
- `offset` advances by the page size asked, not by the items received; rows already loaded (same track ID, album ID, artist ID or playlist UUID; for a playlist's tracks, the same position, as a playlist can hold a track twice: "Bugs") are skipped (the probe's short page, what could go wrong 12)

### Lists and the cursor

Every window has its own cursor, moved by 0004's keys (`j`/`↓`, `k`/`↑`, `g g`, `G`), plus `C-f`/`PageDown` and `C-b`/`PageUp` (a window's height). `G` goes to the last *loaded* row (and so loads the next page). The focused window's cursor is highlighted; the others' are drawn dim. `Tab` focuses the next window, `BackTab` (Shift-Tab) the previous, wrapping. Cursors are by position on browse pages and by entry ID on the queue (0004 AC22, unchanged). A window's first page is fetched when the page opens, except *All tracks*, fetched when first focused.

Rows:

- **Track rows** (favorite tracks, album, playlist, top tracks, all tracks, queue): 0004's queue columns (number, title, artists, album, length). Tracks that are not streamable are drawn dim, as the player will skip them (0004 AC7). *All tracks* adds the artist's role categories (`Performer, Songwriter`) in place of the album column when the window is narrower than 100 columns
- **Album rows**: title, artists, year (and `EP`/`Single` for those)
- **Playlist rows**: title, number of tracks, `♥` for favorite (followed) playlists
- **Artist rows**: name

### The artist's *All tracks* (Tidal's "Credits for <artist>")

From the contributor page's `ITEM_LIST_WITH_ROLES` module and its `dataApiPath` (facts), most popular first, as Tidal orders it. Left out, with the count of hidden rows in the window title (`All tracks (548 · 37 hidden)`):

- tracks without `STEREO` in `audioModes` (Dolby-Atmos-only copies, which the quality ladder does not play)
- **alternate versions**: a track whose `version`, or a bracketed part (`(…)`, `[…]`) or ` - …` suffix of its title, equals one of the **hidden-version words** once both are lower-cased and stripped of spaces, hyphens, underscores, dots and `+`. The words are a setting, `TIDAL_PLAYER_HIDE_VERSIONS` (comma-separated; empty string = hide nothing; until 0008), default `instrumental, inst, off vocal, karaoke, tv version, tv ver, tv size, tv edit, sped up, speed up, nightcore, slowed, slowed + reverb, reverb, 8d, 8d audio`. A word also matches as the start of the part (`TV Size ver.` → `tvsizever` starts with `tvsize`). `Acoustic`, `Live`, `Remix`, `Tiësto Remix` are kept. Other windows and pages show every version

**Role filter** (as tidal.com's credits page, `roleCategoryFilters`): `f` in *All tracks* opens a popup with the four role categories (Performer, Songwriter, Producer, Engineer) as check boxes, all checked at first; `Space` toggles one, `Enter` applies, `Esc` cancels. A track is shown when any of the artist's roles on it is in a checked category. The filter is applied in the client to the loaded rows (the API has no filter parameter: facts), so loading more pages continues as usual; the title says `All tracks (548 · Performer, Songwriter · 37 hidden)`. The filter lasts as long as the page is in the history

### Playing and queueing from a page

| Key | On | Does |
|---|---|---|
| `Enter` | a track (browse page) | Replaces the queue with **every track of that list** (fetching the rest first), in page order, and plays the chosen one: `LoadQueue { tracks, start: index }` (what went wrong 1, 2) |
| `Enter` | a track (queue page) | Plays that entry (0004, unchanged) |
| `Enter` | an album, playlist or artist | Opens its page |
| `Z`, `C-z` | a track | Adds it at the end of the queue: `Open { items: [Track(id)], at: Some(End) }`, so with nothing playing it starts (0005's rule, decision 3) |
| `Z`, `C-z` | an album or playlist | Adds all its tracks at the end: `Open { items: [album / playlist], at: Some(End) }` (the player expands it, 0005) |
| `Z`, `C-z` | an artist | Nothing |
| `d` | an entry (queue page) | Removes it: `RemoveFromQueue(entry ID)` (0004's rules: removing the current entry moves on) |

### Actions popup

spotify-player's actions popup: `g a` or `C-Space` on the selected row, `a` on the playing track. A small list over the page, titled with the item; `j`/`k` move, `Enter` runs the action and closes the popup, `Esc` closes it. While it is open no other key acts.

| Item | Actions, in this order |
|---|---|
| Track (browse page) | *Go to album*, *Go to artist: <name>* (one per artist), *Add to queue*, *Play next*, *Add to favorites* or *Remove from favorites*, *Add to playlist…*, *Remove from this playlist* (on a playlist page of an own playlist) |
| Entry (queue page), or the playing track (`a`) | *Go to album*, *Go to artist …*, *Play next* (not for the playing track), *Remove from queue*, *Add to favorites*/*Remove from favorites*, *Add to playlist…* |
| Album | *Open*, *Go to artist …*, *Add to queue*, *Play next*, *Add to favorites*/*Remove from favorites*, *Add to playlist…* |
| Playlist | *Open*, *Add to queue*, *Play next*, *Add to favorites*/*Remove from favorites* (not for own playlists), *Delete playlist* (own only) |
| Artist | *Open*, *Add to favorites*/*Remove from favorites* |

- *Play next* is *Add to queue* with `at: Next`. *Go to album* is missing for a track without an album; `a` with nothing playing does nothing
- **Favorite or not**: the popup asks the player (`IsFavorite`, one request against the loaded favorites ID list, `GET /users/{user}/favorites/ids`, see "Not verified"; until confirmed, the popup shows both *Add to favorites* and *Remove from favorites*, both harmless when redundant: the write probe shows adding twice and removing a non-favorite both answer `200`)
- ***Add to playlist…*** opens a second popup listing the user's own playlists (`USER_CREATED`, newest first) under a first row *New playlist…*; `Enter` adds the item's tracks (a list: all of them, fetched first) at the end. A track already in that playlist asks `Already in <title>: add again? (y/n)`; `y` adds with `onDupes=ADD`. *New playlist…* opens a one-row prompt `Playlist name: `, creates a private playlist with that title, then adds
- ***Delete playlist*** asks `Delete <title>? (y/n)`
- Results show in the message row: `Added to favorites`, `Removed from favorites`, `Added 12 tracks to <title>`, `Removed from <title>`, `Created <title>`, `Deleted <title>`, or the error. A page that shows the changed list (favorites, the library, the playlist) is fetched again after a successful change

### `Esc` and quitting

As in spotify-player: `Esc` closes the popup or the open prompt (or cancels a whole-list load), and otherwise does nothing; `q` (and `C-c`) quits. This changes 0004, where `Esc` also quit (decision 2; 0004 AC20's row for `Esc` is updated).

### Talking to the player

Browsing and library edits are requests to the player (decision 1), answered to the asking client alone:

- `ClientMessage::Library { id, request }` → `ServerMessage::LibraryReply { id, result: Ok(LibraryResponse) | Err(message) }`
- Reads: `Page(PageRequest)` (`Library`, `FavoriteTracks`, `Album(id)`, `Playlist(uuid)`, `Artist(id)`) → `Page(PageData)`: the page's header and the first page of each of its lists (*All tracks* excepted). `More { list: ListRef, offset, limit }` → `Items(ListPage)`. `ListRef` names one list: `FavoriteTracks`, `Playlists`, `FavoriteAlbums`, `FavoriteArtists`, `AlbumTracks(id)`, `PlaylistTracks(uuid)`, `TopTracks(id)`, `ArtistAlbums(id)`, `ArtistAppearsOn(id)`, `Credits(id)`. `IsFavorite(kind, id)` → `Favorite(bool)`
- Writes: `AddFavorite(kind, id)`, `RemoveFavorite(kind, id)`, `AddToPlaylist { uuid, tracks, allow_duplicates }`, `RemoveFromPlaylist { uuid, index, etag }`, `CreatePlaylist { title }` → `Created(PlaylistSummary)`, `DeletePlaylist { uuid }` → `Done` (or the error). `kind` is track, album, artist or playlist

Rules:

- Requests run as jobs beside the player's input loop (as 0004's suggestions and 0005's `Open` expansions do), so they never delay a command or an event. A client's requests run one at a time, in the order sent; different clients' run independently
- One `LibraryReply` per request, also on failure. It is not an `Event`: other clients do not see it
- A client that disconnects loses its pending replies; a write in flight completes or fails in the player regardless
- Failure messages: 0004's metadata wording (`Album 1 was not found`, `Playlist <uuid> was not found`, `Artist 1 was not found`, `Track 1 was not found`), the session-expired message for `LoginRequired`, `Could not reach Tidal: …`, `Tidal answered 429: try again in a moment`; for playlist edits on `412`/`7002`, `The playlist changed: nothing was changed, try again` (and the playlist page is fetched again). Never retried in a loop
- **Playlist edits and the ETag**: `AddToPlaylist` reads the playlist's current ETag (`GET /playlists/{uuid}`) and sends it; on `412` it reads it once more and retries once (adding at the end is safe to retry). `RemoveFromPlaylist` sends the ETag the client's loaded list came with and **never retries**: a `412` means positions may have moved. `DeletePlaylist` reads the current ETag first

### What the player fetches and writes

All requests go through the `Authenticator` with `countryCode` from the session (0004), `{user}` being the session's `user_id`. Endpoints, envelopes, page sizes and orders are the probes' ("Facts vs. assumptions").

| List / request | API call | Largest page | Kept |
|---|---|---|---|
| `Playlists` | `GET /users/{user}/playlistsAndFavoritePlaylists?order=DATE&orderDirection=DESC` | 50 | `items[].playlist` (UUID, title, `numberOfTracks`, `duration`); **own** when the entry's `type` is `USER_CREATED`, favorite when `USER_FAVORITE` |
| `FavoriteAlbums`, `FavoriteArtists` | `GET /users/{user}/favorites/{albums,artists}?order=DATE&orderDirection=DESC` | 1000 | `items[].item`: albums (ID, title, artists, year from `releaseDate`, `type`, `numberOfTracks`, `duration`), artists (ID, name) |
| `FavoriteTracks` | `GET /users/{user}/favorites/tracks?order=DATE&orderDirection=DESC` | 1000 | `items[].item` (0004's track mapping) |
| `AlbumTracks(id)` | `GET /albums/{id}/tracks` (0004) | 1000 | bare tracks |
| `PlaylistTracks(uuid)` | `GET /playlists/{uuid}/items` (0004, `type == "track"` only) | 100 | `items[].item` |
| `TopTracks(id)` | `GET /artists/{id}/toptracks` | 1000 | bare tracks |
| `ArtistAlbums(id)` | `GET /artists/{id}/albums`, then (when it is exhausted) `…/albums?filter=EPSANDSINGLES` | 1000 | albums, then EPs and singles |
| `ArtistAppearsOn(id)` | `GET /artists/{id}/albums?filter=COMPILATIONS` | 1000 | albums |
| `Credits(id)` | `GET /pages/contributor?artistId={id}&deviceType=BROWSER&locale=en_US` for the first page and the `dataApiPath`; `GET /{dataApiPath}&limit=…&offset=…` after | 50 | `items[].item` and `items[].roles[].category`, filtered as under "All tracks" |
| Page headers | `GET /albums/{id}`, `GET /playlists/{uuid}` (also its `ETag` header), `GET /artists/{id}` | — | album (title, artists, year, tracks, duration), playlist (title, tracks, duration, ETag), artist (name) |
| Favorites | `POST /users/{user}/favorites/{tracks,albums,artists,playlists}` form `trackIds`/`albumIds`/`artistIds`/`uuids`; `DELETE /users/{user}/favorites/{kind}/{id}` | — | — |
| Playlists | `POST /users/{user}/playlists` form `title`, `description=""`; `POST /playlists/{uuid}/items` form `trackIds` (comma list, at most 100 per request: more are sent in order, each with the new ETag), `onDupes=ADD`, `If-None-Match`; `DELETE /playlists/{uuid}/items/{index}` with `If-None-Match`; `DELETE /playlists/{uuid}` with `If-None-Match` | — | — |

A `404`/`2001` on a page's own resource (`/albums/{id}`, `/playlists/{uuid}`, `/artists/{id}`, `/pages/contributor`) is `NotFound`; an unknown artist's `/albums` is a `200` with no items, so the header says it does not exist. A page's header and first pages are fetched in parallel; any failing call fails the page (no half pages). Later pages fail alone (above). Without `order`/`orderDirection` the favorites lists come in no date order, so the parameters are always sent.

### Types (`tidal_player_core`)

- `Track` gains the IDs needed to go to its album and artists: `artists: Vec<ArtistRef { id, name }>` (was names only) and `album: Option<AlbumRef { id, title }>` (was the title only). The mapping fills them from the existing `artists[].id`/`album.id` fields. Rendering is unchanged
- `library` module: `AlbumSummary { id, title, artists, year: Option<u16>, kind: Album | Ep | Single, tracks: Option<u32>, duration: Option<Duration> }`, `PlaylistSummary { uuid, title, tracks: Option<u32>, duration: Option<Duration>, own: bool }`, `CreditedTrack { track, roles: Vec<RoleCategory> }`, `ListPage<T> { items: Vec<T>, offset, total }`, `ListRef`, `PageRequest`, `PageData`, `LibraryRequest`, `LibraryResponse`, `FavoriteKind`, and `hidden_version(&Track, &[String]) -> bool` (the version filter, pure)
- All of them `Serialize`/`Deserialize`, as they cross the socket

### Client model

`tidal_player_core::ui::State` gains the history (`Vec<Page>`, bottom first), each `Page` holding its kind, its header, its windows (each: loaded rows, `total`, cursor, `Idle | Loading { id } | Failed(message)`), its focus; the popup (actions, *Add to playlist…*, a `y/n` question, the name prompt); and a whole-list load in progress. `Effect` gains `Library { id, request }`; `Action` gains `LibraryReply { id, result }`. Request IDs come from a counter in the state (unique per client run).

- While **disconnected** (0005), loaded pages can still be browsed and opened from history, but nothing is requested or sent: opening a new page shows it failed with the disconnected message, and pending loads fail with it. On the next `Welcome`, the page on top, if it failed that way, is fetched again
- When the player says the **session expired**, requests fail with the session-expired message; after `LoginRestored`, opening the page again works

### Rendering

The page below the playback window replaces the queue's area (0004 "TUI" rules hold: the playback window keeps its 4 rows, nothing shares rows with it). A page with several windows draws them side by side when the inner width is at least 60 columns:

- Library: Playlists 40 %, Albums 40 %, Artists 20 % (spotify-player's defaults)
- Artist: two **panes**, each with two **tabs** (lazygit's panels and tabs): the left 60 % holds *Top tracks* and *All tracks*, the right 40 % *Albums* and *Appears on*. A pane shows its active tab; its title lists both, the active one highlighted: `Top tracks (91) │ All tracks  [ ]`. `Tab`/`Shift-Tab` move the focus between panes (keeping each pane's active tab); `[` and `]` switch the focused pane's tab (previous/next, wrapping). On other pages every window is its own pane and `[`/`]` do nothing ("Bugs")

Below 60 columns only the focused pane is drawn, its title followed by `‹Tab›` when there are other panes. The title row takes one row above the windows. Popups are centred over the page, at most 50 columns wide, clipped to the page. Rows are truncated with `…` as in 0004; nothing panics at any size (0 × 0 included).

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Library                                                                       │
│┌Playlists (22)──────────────┐┌Albums (14)─────────────────┐┌Artists (196)───┐│
││Running                  42 ││Collide With The Sky  Pier…  ││Pierce The Veil ││
││♥ Late night             17 ││Misadventures         Pier…  ││Sleeping With S…││
│…                                                                             │
```

## Acceptance criteria

Protocol and types (`tidal_player_core`, pure):

- **AC1** — `ClientMessage::Library`, `ServerMessage::LibraryReply`, every `LibraryRequest`, `LibraryResponse`, `PageRequest`, `PageData` and `ListRef` variant, the summaries, `CreditedTrack`, `ListPage` and the new `Track` shape survive a JSON round trip (0001 AC11's test, extended); 0005's codec decodes both new messages
- **AC2** — `Track`'s new fields: the 0004 metadata mapping fills `artists[].id` and `album.id` from the existing fixtures; `album: null` maps to `None`. Every place that showed artist names and the album title shows the same text as before (0004/0005 rendering snapshots unchanged)
- **AC3** — `hidden_version` (table, default words): hidden — `version: "Instrumental"`, `"TV Size"`, `"TV-size ver."`, `"Sped Up"`, `"Slowed + Reverb"`, `"Off Vocal"`, `"Nightcore"`, titles `"X (Instrumental)"`, `"X [TV Size]"`, `"X - Sped Up Version"` (`spedupversion` starts with `spedup`), `"X (slowed & reverb)"` with the word list extended by `slowed & reverb`; kept — `"Acoustic"`, `"Live"`, `"Tiësto Remix"`, `"Remastered 2011"`, a title containing `Instrumental` outside brackets (`"Instrumentally Yours"`), `None`; an empty word list hides nothing. Settings: `TIDAL_PLAYER_HIDE_VERSIONS` and `TIDAL_PLAYER_PAGE_SIZE` in `resolve_player_config` (0004 AC25's table, extended: unset, empty, `1`, `10000`, `0`, `10001`, `x`)

API (`tidal-player-api::library`, wiremock fixtures written from the probes):

- **AC4** — Reading lists: for each `ListRef`, one request per `More` with the path, `countryCode`, the order parameters where listed, `limit = min(asked, largest page)` (asking 100 of playlists or credits sends 50) and the given `offset`; the envelope unwrapped (`items[].item`, `items[].playlist` with `own` from `USER_CREATED`/`USER_FAVORITE`, bare items); `total` from `totalNumberOfItems`; non-track playlist items dropped. `ArtistAlbums` continues with `filter=EPSANDSINGLES` from offset 0 once the plain list is exhausted, with `total` the sum (table over every `ListRef`)
- **AC5** — Pages: `Library`, `FavoriteTracks`, `Album`, `Playlist`, `Artist` return the header and each list's first page from parallel requests; the playlist's `ETag` header is kept; `404`/`2001` on the header (and on `/pages/contributor`) → `NotFound` with the item's message; an unknown artist whose `/albums` answers `200` empty still fails from `/artists/{id}`; any failing call fails the page
- **AC6** — Credits: the first page from `/pages/contributor` (the `ITEM_LIST_WITH_ROLES` module's `pagedList`), later pages from its `dataApiPath` with `limit`/`offset` appended; role categories mapped; tracks without `STEREO` dropped and alternate versions hidden per `hidden_version`, with the hidden count reported; a page with no such module → `No credits`
- **AC7** — Writes: `AddFavorite`/`RemoveFavorite` send the form field per kind (`trackIds`, `albumIds`, `artistIds`, `uuids`) and the `DELETE` path; `CreatePlaylist` posts `title`; `AddToPlaylist` reads the ETag, sends it as `If-None-Match` with `trackIds` comma-joined (250 tracks: 3 requests, each with the ETag the previous answered) and `onDupes=ADD` only when allowed, and on one `412` re-reads and retries once (a second `412` → the changed-playlist message); `RemoveFromPlaylist` sends the given ETag and position and on `412` returns the changed-playlist message without retrying; `DeletePlaylist` reads the ETag and sends it; `LoginRequired` and transient errors returned unchanged by every call (table)

Player (`tidal-player`, fake library source, 0005's server tests):

- **AC8** — Each `Library` request is answered with exactly one `LibraryReply` with the same `id`, to that client only (another subscriber receives nothing); errors become `Err(message)` with the messages under "Talking to the player" (table); while a request is held pending by the fake, the same client's `Request { TogglePause }` is replied within 1 s and events keep flowing; one client's second request starts after its first answered, while another client's proceeds independently; a client that disconnects mid-request leaves the player unaffected

Client model (`tidal_player_core::ui`, pure):

- **AC9** — History: `z`, `g l`, `g y` and `Enter` on album/playlist/artist rows push the page and emit `Library { Page(…) }` with a fresh ID (the queue page emits none); opening the top page again does nothing; `Backspace`/`C-q` pop and the page under shows its kept rows, cursors and focus with no request; the bottom page is never popped; the 51st push drops the oldest page above the queue. Start: `start_on_library` puts the library over the queue showing `Loading…` and emits nothing; the first `Welcome` emits its `Page(Library)` request once (a second `Welcome` does not); a `Disconnected` before it fails the page with the disconnected message and the next `Welcome` fetches it. A reply applies to the page/window with that ID only, also when it is not on top; one for an unknown ID is dropped (table)
- **AC10** — Scrolling: a cursor move that comes within one window height of the last loaded row emits `More { list, offset: loaded so far rounded up to the page size, limit: page size }` once (no second request while one is pending); its rows are appended skipping known IDs; a short page before the total does not stop later loads; a failed `More` shows its message as the last row and the next move near the end asks again; `G` goes to the last loaded row; *All tracks* asks for its first page when first focused; the role filter (`f`, `Space`, `Enter`, `Esc`) shows only rows with a checked category and keeps the hidden count, and asks for more pages when the filtered rows run out near the cursor (table)
- **AC11** — Windows and cursors: `Tab`/`BackTab` cycle the focus over the page's windows, wrapping; cursor keys move the focused window's cursor only, clamped to the loaded rows; cursors survive leaving and coming back; a loading, failed or empty window ignores them
- **AC12** — Playing and queueing (table over every row of "Playing and queueing from a page"): `Enter` on track *i* of a fully loaded *n*-track list sends `LoadQueue { tracks: the list in page order, start: i }` (never sorted or shuffled); on a partly loaded list it first emits `More` until the total is loaded, then sends it once; `Esc` while loading cancels and sends nothing; above 40 000 tracks it sends nothing and sets the too-many message; `Z`/`C-z` on a track sends `Open { [Track(id)], Some(End) }`, on an album/playlist `Open { [item], Some(End) }`, on an artist nothing; `d` on the queue sends `RemoveFromQueue(entry ID)` and on a browse page nothing; the queue's `Enter` is 0004's
- **AC13** — Actions popup: `g a`/`C-Space` open it for the selected row and `a` for the playing track, with the actions per item kind and order of the table (table, incl. a track with two artists, one without an album, an own and a followed playlist, a track on an own playlist page; `a` with nothing playing opens nothing); `j`/`k` move, `Enter` emits the action's effect (`Go to …` pushes and fetches that page; *Play next* sends `at: Next`; favorites send `AddFavorite`/`RemoveFavorite` with the right kind and ID) and closes it; `Esc` closes it with no effect; while open, no other key acts (`Space`, `n`, `q` included)
- **AC14** — Playlist editing in the model: *Add to playlist…* lists own playlists only, *New playlist…* first; choosing one sends `AddToPlaylist` (a list: fetched whole first); a track already in a loaded copy of that playlist asks `y/n` and `y` sends `allow_duplicates: true`, `n` sends nothing; *New playlist…* takes a name (empty name: nothing) and sends `CreatePlaylist`, then `AddToPlaylist` to the created UUID; *Remove from this playlist* sends the row's position and the page's ETag; *Delete playlist* asks `y/n`; each success sets its message and re-fetches the affected page if it is in the history; an error sets the message and changes nothing (table)
- **AC15** — `Esc` with no popup, prompt or whole-list load emits nothing (0004 AC20's `Esc` row changed from `Quit`); `q` and `C-c` emit `Quit`
- **AC16** — Disconnected and login (table): while disconnected, opening a page shows it failed with the disconnected message and emits no request, a pending load fails with it, history and cursors work, `Enter`/`Z`/`d` and the popup's actions emit nothing; the first `Welcome` after that re-fetches the top page if it failed that way (and only it); a session-expired error is shown in the page

TUI rendering and runtime (`tidal-player`):

- **AC17** — Rendering (`insta`, 80×24, reviewed by eye, plus `contains` checks): library loaded (three windows, counts, focus, `♥`); library loading; favorite tracks with a non-streamable row dimmed and `Loading more…` as the last row; album page title row; artist page (both halves, `‹Tab›` titles, hidden count); a failed page; each empty-list message; the actions popup; *Add to playlist…*; a `y/n` question. 50×20: only the focused window with `‹Tab›`. No panic from 0×0 to 120×40 on every page and popup (table over sizes × pages)
- **AC18** — Key decoding: `Tab`, `BackTab`, `Backspace`, `PageUp`/`PageDown`, `C-Space`, `C-f`/`C-b`/`C-q`/`C-z`/`C-c` decode to the model's keys (`crates/app/src/input.rs`, table)
- **AC19** — Wiring: the TUI client runtime sends `Effect::Library` as `ClientMessage::Library` and turns `LibraryReply` into `Action::LibraryReply`, both attached and standalone (in-process link, 0005); an attached client loading a page still opens no session store, keyring or passphrase source (0005 AC14's test, with a `g l` answered from the test player's wiremock API)

Docs:

- **AC20** — `docs/tui.md` documents the pages, the history, the windows, scrolling loads, playing and queueing from a page, the actions popup, favorites and playlist editing, the new keys and the changed `Esc`, the two settings, and the empty/failure messages; `docs/playback.md` "Settings" lists the two settings; `CLAUDE.md` "Status" names this spec; this spec links to them

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| Thousands of favorite tracks | 100 rows load at once, more as you scroll; playback and other keys keep working (AC8, AC10) |
| `Enter` on track 3 of a 5 000-track list | The rest loads first (`Loading 300 of 5 000…`, `Esc` cancels), then one `LoadQueue` of 5 000 (about 1.5 MB on the socket) |
| The user opens pages faster than they load | Each request has its own ID; results land on their own page or are dropped (AC9); a client's requests run one at a time in the player (AC8), so mashing does not burst requests at Tidal |
| `429` while loading | The page or the `Loading more…` row shows the message; nothing retries in a loop (0004 "What went wrong" 3) |
| A favorite album or playlist removed from Tidal | Listed as Tidal returns it; opening it shows `Album 1 was not found` |
| A track unavailable in the user's country | Shown dimmed; queued, then skipped by the player as a track-only failure (0004 AC7) |
| A playlist of only videos | `This playlist has no tracks`; `Z` on it: `Playlist <uuid> has no tracks` (0004) |
| An artist with no albums, top tracks or credits | That window says so |
| *All tracks* where most rows are hidden | The title says how many are hidden; an empty result after filtering says `No credits (37 hidden)` |
| A track without an album (`album: null`) | No *Go to album* action |
| The playlist was edited elsewhere (another app) before a removal | `412`: `The playlist changed: nothing was changed, try again`; the page reloads; nothing removed by a stale position |
| Adding a duplicate to a playlist | Asked first (`y/n`); Tidal itself would accept it |
| Session expires while browsing or editing | Requests fail with the session-expired message; the status line shows it (0002 AC14) |
| The player shuts down or the connection drops while a page loads | The page fails with the disconnected message and is fetched again on reconnect if still on top (AC16) |
| Version mismatch between client and player | Refused at the greeting (0005), as before |
| Narrow terminal | One window at a time (AC17); below 6 inner rows only the playback window (0004) |

## Test plan

Each automated test is named after its criterion (`ac10_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0005. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/library/`, written from the probes' recorded shapes (IDs and names replaced by fakes; the write probe's ETag and `412` bodies as recorded). Server tests reuse 0005's harness with a fake library source that can hold a request pending.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (extended) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | new types' hand-written stub `Serialize` writes `null` |
| AC2 | `crates/api/tests/metadata.rs` :: `ac2_track_refs` | IDs mapped; `album: null` → `None` | stub mapping leaves `id` 0 |
| AC3 | `crates/core/src/library.rs` :: `ac3_hidden_version` (table) + `crates/app/src/play.rs` :: `ac25_player_config` (new rows) | hidden or kept per row; settings | stub hides nothing |
| AC4 | `crates/api/tests/library.rs` :: `ac4_lists` (table over `ListRef`) | path, params, clamped `limit`, unwrapping, `total` | stub ignores `offset` and the clamp |
| AC5 | `crates/api/tests/library.rs` :: `ac5_pages` (table) | header + first pages; ETag; errors | stub returns an empty header |
| AC6 | `crates/api/tests/library.rs` :: `ac6_credits` | first page and `dataApiPath` pages; filters; hidden count | stub keeps Atmos-only and instrumental rows |
| AC7 | `crates/api/tests/library.rs` :: `ac7_writes` (table) | forms, paths, `If-None-Match`, retry once on add, no retry on remove | stub sends no ETag, so every edit is a `412` |
| AC8 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender`, `ac8_errors` (table), `ac8_does_not_block` | one reply, sender only, messages, latency, per-client order | stub runs the request on the player thread, so a held one blocks the `TogglePause` reply |
| AC9 | `crates/core/src/ui.rs` :: `ac9_history` (table), `ac9_reply_by_id` (table), `ac9_start_on_library` | stack, effects, kept state, stale drop | stub replaces the page instead of pushing |
| AC10 | `crates/core/src/ui.rs` :: `ac10_scroll_loads` (table) | `More` offsets, no duplicates, short pages, failures | stub never asks for a second page |
| AC11 | `crates/core/src/ui.rs` :: `ac11_windows_and_cursors` (table) | focus cycle, cursor per window | stub moves every window's cursor |
| AC12 | `crates/core/src/ui.rs` :: `ac12_play_and_queue` (table) | the exact command per row; whole-list load | stub `Enter` sends the loaded rows only |
| AC13 | `crates/core/src/ui.rs` :: `ac13_actions_popup` (table) | actions per kind, effects, closing, key capture | stub popup lists *Add to queue* only |
| AC14 | `crates/core/src/ui.rs` :: `ac14_playlist_editing` (table) | requests, questions, messages, re-fetches | stub adds duplicates without asking |
| AC15 | `crates/core/src/ui.rs` :: `ac15_esc_and_quit` + 0004's `ac20_keys_to_commands` (row updated) | `Esc` → nothing; `q`/`C-c` → `Quit` | stub keeps `Esc` → `Quit` |
| AC16 | `crates/core/src/ui.rs` :: `ac16_disconnected_and_login` (table) | no request while disconnected; one re-fetch | stub requests while disconnected |
| AC17 | `crates/app/src/ui.rs` :: `ac17_pages_80x24` (one snapshot per row), `ac17_narrow_50x20`, `ac17_no_panic_any_size` | `contains` checks + snapshots | stub render draws the queue whatever the page |
| AC18 | `crates/app/src/input.rs` :: `ac18_key_events` (table) | decoded keys | stub maps the new keys to nothing |
| AC19 | `crates/app/src/client.rs` :: `ac19_library_round_trip` (attached and in-process) + `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (extended) | messages both ways; no session opened | stub client drops `Effect::Library` |
| AC20 | — reviewed at acceptance | docs match this spec, linked | — |

Not covered by automated tests, on purpose, and checked by hand at acceptance with a real account (results in the PR description):

- `g l` lists the same playlists, albums and artists (and counts) as the Tidal app; `g y` the same favorite tracks, in the same order; scrolling to the end of the 362 favorites loads them all, without duplicates
- A large list loads while a track plays, without a hiccup in playback or the keys
- `Enter` in a playlist plays it from that track with the rest queued; `Z` on an album in the library adds it; *Go to artist* from the playing track opens the artist; *All tracks* for an artist with instrumental and sped-up releases hides them
- Add and remove a favorite, create a playlist, add tracks, remove one, delete it: each shows in the Tidal app

## Crate placement

- `tidal-player-core`: `library` (summaries, refs, `ListPage`, requests and responses, `hidden_version`), `track` (`ArtistRef`, `AlbumRef`), `protocol` (`Library`, `LibraryReply`), `ui` (pages, history, windows, scrolling loads, popups, `Effect::Library`, `Action::LibraryReply`, the new keys). No I/O, no new dependency
- `tidal-player-api::library`: the reads and writes and their DTO → core mapping; `MetadataError::NotFound` takes the new `Artist` kind; the ETag header read
- `tidal-player`: `ipc::server` (`Library` → a job; `LibraryReply` to that client's outbox), `player_runtime` (a `Library` trait beside 0004's `Metadata`, implemented by the API client and faked in tests), `play.rs` settings, `client.rs` (the effect and the action), `ui.rs` (pages, windows, popups), `input.rs` (new keys)
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-07, from code and docs):

- The session holds `user_id` and `country_code` (`crates/api/src/auth.rs`, `Session`)
- The track DTO already deserialises from objects with `artists[].id` and `album.id` (0004 "Facts"; `crates/api/tests/fixtures/metadata/track.json`)
- The player runtime already runs fetches as jobs beside its input loop (`RuntimeInput::Expanded`, `RuntimeInput::Suggestions` in `crates/app/src/player_runtime.rs`), and the server can send a message to one client (`Hub::reply`)
- spotify-player's default keys (its `README.md` "Commands" table, read 2026-10-07): `g l` library, `g y` liked tracks, `z` queue, `Backspace`/`C-q` previous page, `Tab` next window, `g a`/`C-Space` actions on the selected item, `a` actions on the current track, `Z`/`C-z` add the selected item to the queue, `Esc` closes a popup, `q`/`C-c` quit; its library page has Playlists, Albums and Artists windows, in that order, at 40/40/20 % (`spotify_player/src/ui/page.rs` `render_library_page`, `docs/config.md` `library.*_percent`); its artist page has top tracks, albums and related artists

Verified on 2026-10-07 against the **live API** by `scripts/tidal-library-probe.sh`, run by the user (same account and client as 0004's probes: 22 playlists, 362 favorite tracks, 14 albums, 196 artists). Fixtures under `crates/api/tests/fixtures/library/` are written from these shapes with IDs and names replaced:

- `GET /v1/users/{user}/playlistsAndFavoritePlaylists` → `{limit, offset, totalNumberOfItems, items: [{type, created, playlist}]}`, `type` `USER_CREATED` (own, 10) or `USER_FAVORITE` (12, matching `/favorites/playlists`' total); `playlist` has `uuid`, `title`, `numberOfTracks`, `numberOfVideos`, `duration` (seconds), `type` (`USER`, `EDITORIAL`), `created`, `lastUpdated`, `creator`, `description` and image fields. Default page 10; **`limit=1000` → `400`/`1001` `Too big page, max page size is [50]`**; `limit=3&offset=2` pages as expected. With `order=DATE&orderDirection=DESC` the entries' `created` (the date added, for favorites) is strictly newest first. `/users/{user}/playlists` (tidalt's) lists only the 10 own playlists, bare; `/favorites/playlists` only the 12 favorites, as `{created, item}`
- `GET /v1/users/{user}/favorites/tracks` → `{limit, offset, totalNumberOfItems, items: [{created, item: track}]}`, `item` the full track of 0004's fixtures (with `artists[].id`, `album.id`, `album: {id, title, cover, …}`); no `type` field and no video entries (every item a track). Default page 10 and **not in date order**; `order=DATE&orderDirection=DESC` newest first, `ASC` oldest first. `limit=1000` and `limit=10000` are accepted and returned **358 items for `totalNumberOfItems: 362`**
- `GET /v1/users/{user}/favorites/albums` → `{created, item: album}` entries; albums have `id`, `title`, `artists[]`, `releaseDate` (`YYYY-MM-DD`), `numberOfTracks`, `numberOfVolumes`, `duration`, `type` (`ALBUM`, `SINGLE` seen), `allowStreaming`/`streamReady`. Default page 10; `limit=1000` accepted; DESC order newest first
- `GET /v1/users/{user}/favorites/artists` → `{created, item: {id, name, picture, artistTypes, mixes, …}}`. Default page 10; `limit=1000` returned all 196; DESC newest first (many entries share one `created`, from a bulk import; their order among themselves is the API's)
- `GET /v1/users/1/favorites/tracks` (another user's ID) answered `200` with **this account's** 362 favorites: the path's user ID is not checked against the token. The session's `user_id` is still the one sent
- `GET /v1/albums/{id}` → the album object above; unknown → `404 {"subStatus":2001,"userMessage":"Album [1] not found"}`. `GET /v1/playlists/{uuid}` → the playlist object above; unknown → `404`/`2001` `Playlist not found`
- `GET /v1/artists/{id}` → `{id, name, picture, artistTypes, artistRoles, mixes, …}`; unknown → `404`/`2001` `Artist [1] not found`. `GET /v1/artists/1/albums` (unknown artist) → `200` with `totalNumberOfItems: 0`
- `GET /v1/artists/{id}/toptracks` → bare tracks; default page 10, `totalNumberOfItems: 91`; `limit=100` returned all 91; `limit=1000` accepted
- `GET /v1/artists/{id}/albums` → bare albums, default page 10, `limit=1000` accepted; without `filter` only `type: ALBUM` (12). `filter=EPSANDSINGLES` → `EP` and `SINGLE` (66). `filter=COMPILATIONS` → the "appears on" albums (30, mostly by `Various Artists`, of every type). The same title can appear twice with different IDs (two `DISASTERPIECE` releases differing in `mediaMetadata.tags`); both are kept, as Tidal lists them
- `GET /v1/artists/{id}/similar` → `{limit, offset, totalNumberOfItems, items: [artist + relationType: "SIMILAR_ARTIST"], source: "TiVo"}` (4 for this artist)

Verified on 2026-10-07 against the **live API** by `scripts/tidal-library-write-probe.sh`, run by the user (everything it added was removed again; the probe playlist was deleted):

- **Favorites**: `POST /v1/users/{user}/favorites/{tracks|albums|artists|playlists}?countryCode=…` with a form body `trackIds` / `albumIds` / `artistIds` / `uuids` → `200`, empty body, an `ETag` header. Adding again is `200` too (no duplicate: the total grew by one). A comma list (`trackIds=a,b`) adds both. The added item comes first in the DATE DESC list. `DELETE /v1/users/{user}/favorites/{kind}/{id}` → `200`, also when it is not a favorite. An unknown track → `404`/`2001` (with the message `Album not found`, so the message is not shown as is)
- **Create**: `POST /v1/users/{user}/playlists?countryCode=…` with form `title`, `description` → `201` and the playlist object (`uuid`, `numberOfTracks: 0`, `publicPlaylist: false`), `ETag` header
- **Edit needs the current ETag**: `GET /v1/playlists/{uuid}` returns `ETag: "<lastUpdated ms>"`; every edit sends it as `If-None-Match`. Without it, or with an outdated one, `412 {"subStatus":7002,"userMessage":"You must send the correct Etag value in the If-None-Match header to modify a playlist"}`. Each successful edit answers the new ETag
- **Add**: `POST /v1/playlists/{uuid}/items` with form `trackIds` (comma list), `onDupes` (`FAIL`/`ADD`), optional `toIndex` → `200 {"lastUpdated", "addedItemIds"}`; `toIndex=0` inserted at the top. Whether `onDupes=FAIL` refuses a track already in the playlist was not shown (the probe's "duplicate" was not one yet); the app sends `onDupes=ADD` only when the user confirms adding a duplicate, else it checks the loaded list itself
- **Remove**: `DELETE /v1/playlists/{uuid}/items/{index}` (0-based position) with the ETag → `200`; the same request with the previous ETag → `412`. Positions shift after each removal, so a removal is addressed by position *and* checked against the ETag the list was loaded with: on `412` the app reloads the playlist and says `The playlist changed: removed nothing, try again`, never removing by a stale position
- **Delete**: `DELETE /v1/playlists/{uuid}` with the ETag → `204`; then `GET` → `404`/`2001`
- Someone else's playlist without an ETag → `412`/`7002`, so ownership was not tested; *Add to playlist* lists only the user's own playlists (`USER_CREATED`), so it never tries

Verified on 2026-10-07 by the user's capture from tidal.com and `scripts/tidal-artist-tracks-probe.sh` (artist 7367609, Dean Lewis):

- `GET /v1/pages/contributor?artistId={id}&countryCode=…&deviceType=BROWSER&locale=en_US` → `200` on `api.tidal.com`, with or without a token: `{title: "Credits", rows: [{modules: [CONTRIBUTOR_HEADER {title: "Credits for Dean Lewis", artist}]}, {modules: [ITEM_LIST_WITH_ROLES {pagedList: {dataApiPath: "pages/data/<uuid>?artistId={id}", limit: 50, offset: 0, totalNumberOfItems: 548, items: [{item: track, type: "track", roles: [{name, category, categoryId}]}]}, roleCategories, …}]}]}`; most popular first. Unknown artist → `404 {"subStatus":2001,"userMessage":"Not found"}`
- `GET /v1/{dataApiPath}&countryCode=…&deviceType=BROWSER&locale=en_US&limit=50&offset=50` → the next 50 (`{limit, offset, totalNumberOfItems: 548, items}`); **`limit` above 50 → `400`/`1001` `Too big page, max page size is [50]`**
- `roleCategoryId=11` is ignored (same total, same first IDs): there is no server-side role filter. The user's capture of tidal.com choosing role categories (artist 7514330) shows the web player calling the same unfiltered `/pages/contributor` URL; the choice lives only in the page address (`/credits/{id}?roleCategoryFilters=2,11`), so the web filters the list itself (or with a later request that was not captured)
- In the first 100 items: 4 `["DOLBY_ATMOS"]`-only tracks (e.g. a `LOW`-quality copy of "Memories"), 1 `["STEREO","DOLBY_ATMOS"]`, the rest `["STEREO"]`; versions seen: `null`, `Acoustic`, `Tiësto Remix`; songwriting- and production-only credits on other artists' tracks

Not verified:

- `GET /users/{user}/favorites/ids` (the favorites ID lists the popup would use to know whether an item is a favorite): not probed. Until it is, the popup shows both *Add* and *Remove* (both harmless when redundant); confirmed in implementation against a fixture only if the user records it
- Whether the dataApiPath's `<uuid>` is stable across sessions (it is re-read from `/pages/contributor` on every page open, so it does not matter)
- Whether the short page (358 of 362) is the server dropping tracks unavailable in the country, and whether it happens on pages other than the last (the walk handles both, AC4)
- Page sizes above 1000 on the favorites albums/artists and artist-albums endpoints (not needed: the walk uses 1000)
- The order the Tidal apps themselves show the library in
- Whether 0004's album and playlist walks (`offset` advanced by the items received) meet the same short pages on `/albums/{id}/tracks` or `/playlists/{uuid}/items`. If they do, they would request overlapping pages and queue a track twice; this spec leaves them as they are, and a 0004 "Bugs" entry follows if it is seen

Verified from tidalt's code and history (2026-10-07): the list under "What tidalt did". Not verified: whether tidalt's single-request lists were actually cut short on a real account (inferred from the `limit` values; no issue reports one)

## Decisions (answered by the user, 2026-10-07; folded into the body above)

1. **Where pages are fetched**: *in the player* (`Fetch`/`Fetched`), as proposed
2. **`Esc`**: *as proposed*: closes the popup or the prompt only; `q`/`C-c` quit (0004 AC20's `Esc` row changes)
3. **`Z` on a track with nothing playing**: *start it*, sent as `Open { items: [Item::Track(id)], at: Some(End) }`
4. **Favoriting and playlist editing**: *in this spec* (2026-10-07: "why not", then "yes" to playlist editing). Actions popup: *Add to favorites*/*Remove from favorites* for tracks, albums, artists and playlists; *Add to playlist…* (pick one of your own playlists, or a new one), *Remove from playlist* on a track of your own playlist, *New playlist*, *Delete playlist* (own). Shapes, ETags and error codes from `scripts/tidal-library-write-probe.sh` ("Facts")
5. **Lists load as you scroll** (2026-10-07: "can do inf-fetching if they do"): Tidal pages with `limit`/`offset` (= page size/page index × size) and returns `totalNumberOfItems` (the read probe). So a page fetches its first page of *N* items (the **page size**, a setting, default **100**, 1–10 000, clamped per endpoint: 50 for playlists) and shows the total at once (`Favorite tracks · 362 tracks`); the next page is fetched when the cursor comes within one window height of the last loaded row (`Loading more…` as the last row). Playing or queueing a whole list (`Enter` on a track, `Z`/*Add to playlist* on a list) first fetches the rest, then sends it. No 10 000-item cap on lists any more; `Enter` on a list longer than the socket's 16 MiB line can hold (about 50 000 tracks) is refused with a message (the body sets the limit at 40 000 tracks)
6. **Artist page**: windows **Top tracks**, **Albums** (albums, then EPs and singles), **Appears on** (`filter=COMPILATIONS`), and **All tracks** = the Tidal apps' **"Credits for <artist>"** (the user, 2026-10-07). The user captured tidal.com's call: `GET /v1/pages/contributor?artistId={id}&countryCode=…&locale=en_US&deviceType=BROWSER` → a page whose second row holds an `ITEM_LIST_WITH_ROLES` module with `pagedList {dataApiPath: "pages/data/<uuid>?artistId={id}", limit: 50, offset: 0, totalNumberOfItems: 548, items: [{item: track, type: "track", roles: [{name, category, categoryId}]}]}`, most popular first, and `roleCategories` Performer (11), Songwriter (2), Producer (1), Engineer (3). It includes tracks where the artist is only credited as songwriter or producer, remixes and acoustic versions, and a Dolby-Atmos-only copy of a track (`audioModes: ["DOLBY_ATMOS"]`, `audioQuality: LOW`). *All tracks* pages through `dataApiPath` as decision 5 does, shows each track's role categories, keeps the API's order, and drops: duplicates (same track ID), tracks without `STEREO` in `audioModes`, and alternate versions (below). `scripts/tidal-artist-tracks-probe.sh` confirmed it on `api.tidal.com`, pages of at most 50 through `dataApiPath`, and no role filter ("Facts")

   **Alternate versions are left out of *All tracks*** by a configurable list of words (2026-10-07: instrumental, then "TV Version, Speed Up, Slowed + Reverb, etc."; a setting, `TIDAL_PLAYER_HIDE_VERSIONS`, comma-separated, until 0008 moves it to `app.toml`). A track is left out when its `version`, or a bracketed part or ` - ` suffix of its title, matches one of the words after both are lower-cased and stripped of spaces, hyphens, underscores, dots and `+`. Default: `instrumental`, `inst`, `off vocal`, `karaoke`, `tv version`, `tv ver`, `tv size`, `tv edit`, `sped up`, `speed up`, `nightcore`, `slowed`, `slowed + reverb`, `reverb`, `8d`, `8d audio`. Table-tested (`TV-size`, `TV Size ver.`, `(Sped Up)`, `Slowed + Reverb`, `[Instrumental]`, `- Off Vocal`; and `Acoustic`, `Tiësto Remix` kept). Other windows show every version

## Out of scope

- Search (0007)
- Mixes, radio and "My mixes" pages (0011); spotify-player's *GoToRadio*
- Renaming playlists, editing their descriptions, reordering their tracks, playlist folders; making a playlist public
- Caching pages or images, remembering the page history across runs (0009 left both out)
- Cover art on pages (0004's placement rule applies when it comes)
- Sorting and filtering lists (spotify-player's `s t`, `/` in a page), the help popup (`?`), configurable keys and page percentages (0008)
- Jumping to the playing track in its list (spotify-player's `g c`), the "currently playing context" page (`g space`)
- One-shot `playback` commands for the library (e.g. `playback load --favorites`)

## Implementation notes (choices made where the spec was silent, 2026-10-07)

- **Types**: IDs are plain `u64` (as `Item::Album`); `Track` gained `version: Option<String>` for the version filter. `ListPage` carries `hidden` (rows a page dropped; non-zero for credits only), and `LibraryResponse::Items` wraps a `ListItems` enum per row type. Favorite requests take `(FavoriteKind, String)`: the ID in decimal or the playlist UUID. An `artists[]` or `album` object without an `id` is malformed
- **Filter**: a hidden-version word that normalises to nothing matches nothing (else it would hide every track)
- **API**: `ArtistAlbums` maps one offset space over two lists: offsets below `ceil(albums / limit) × limit` read the plain list, the rest `EPSANDSINGLES` from 0; each `More` asks the plain list first for its total (two requests). Credits cache each artist's `dataApiPath` and re-read `/pages/contributor` once on a `404`. `AddToPlaylist` retries once per 100-track chunk, so a failure midway can leave earlier chunks added. The playlist header's `own` is `creator.id == user_id` (the probes redacted `creator`: to confirm by hand). `IsFavorite` reads `GET /users/{user}/favorites/ids` with an assumed shape; the client does not send it yet (both favorite actions are shown)
- **Player**: library requests queue per client in the runtime; a client's next request starts when its previous reply is sent
- **Client model**: the list height comes from `Action::Resize { list_height }` (default 20), sent by the TUI whenever the layout's list height changes; a next page loads when `cursor + list_height + 1 ≥ visible rows`. Re-opening the top page re-fetches it only if it failed. Failure text is `Could not load the library: …` / `Could not load the favorite tracks: …`, else the player's message as is. During a whole-list load only `Esc` and `q`/`C-c` act. *Add to playlist…* fetches the user's playlists fresh and checks duplicates against any loaded copy of that playlist in the history. A removal without an ETag sends an empty one (the `412` path handles it). With the role filter on, `Enter` plays the visible tracks. `C-f`/`C-b` also move the queue cursor; `Z` on the queue does nothing
- **Rendering**: a window title too narrow for `‹Tab› <other>` drops the other window's name first. Album rows show `year EP/Single` from 44 inner columns, the year from 30, title and artists from 20. *All tracks* shows the role categories in place of the album column, which exists only from about 120 terminal columns. Shift+Tab is decoded as `BackTab` whether crossterm reports `BackTab` or `Tab` with Shift. `TIDAL_PLAYER_PAGE_SIZE` is read by every TUI, attached or standalone

## Bugs

- **A playlist that holds the same track twice shows it once (found at slice D acceptance, 2026-10-07).** Expected: a playlist row for every item Tidal lists, duplicates included (the write probe added the same track twice to one playlist, and Tidal kept both); *Remove from this playlist* addresses the right one by position. Actual: appending a page skips "rows already loaded (same track ID …)", so the second copy never shows. Root cause: "Lists load as you scroll" made skipping by ID the guard against overlapping pages for every list; for a playlist an ID is not unique. Fix: playlist tracks are skipped by **position** (the item's offset in the list) instead of by ID; every other list keeps skipping by ID. Test: `crates/core/src/ui/page.rs` :: `ac10_playlist_duplicates_kept` (two pages of a playlist with the same track at positions 0 and 1, and a repeated page: both copies kept once each, positions 0 and 1; a favorite-tracks window still drops a repeated ID). Red: the playlist window keeps one row
- **`Tab` on the artist page skips to the other half (reported by the user, 2026-10-08).** Expected: the left half's title `Top tracks ‹Tab› All tracks` means `Tab` shows *All tracks*. Actual: `Tab` cycles the four windows in a fixed order (Top tracks → Albums → Appears on → All tracks), so from *Top tracks* it moves to *Albums* on the right, and there is no way to switch a half between its two windows without passing the other half. Root cause: "Rendering" put two windows in each half but kept `Tab` as one flat focus cycle, and the title promised a switch that key does not do. Fix: the artist page has two panes of two tabs; `Tab`/`Shift-Tab` cycle panes, `[`/`]` cycle the focused pane's tabs (lazygit), titles list a pane's tabs with the active one highlighted and `[ ]`. AC11 gains the rows. Tests: `crates/core/src/ui/browse/tests.rs` :: `ac11_artist_panes_and_tabs` (Tab from *Top tracks* focuses the right pane on its active tab and back again keeps *Top tracks*; `]` on the left pane shows *All tracks* (and asks its first page); `[`/`]` wrap; on the library they do nothing) and the 0006 rendering snapshots of the artist page (titles). Red: `Tab` from *Top tracks* lands on *Albums* but `]` does nothing
