# 0006 — Library: favorites, playlists, album and artist pages

- **Status**: draft (2026-10-07; probe recorded, decisions answered; waiting on the writing and artist-tracks probes; the body is rewritten to decisions 2–6 after it)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0002 (implemented: the session's `user_id` and `country_code`), 0004 (implemented: the queue, `LoadQueue`/`AddToQueue`, the TUI), 0005 (implemented: the socket, the client/player split, `Open`)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Pages", "The library", "Album, playlist and artist pages" and "Actions" sections and the new keys (AC17)

## Context

Until now the queue can only be filled from pasted links and IDs (0004 decision 1). This spec adds what spotify-player calls **pages**: the user's library (their playlists, favorite albums and favorite artists), their favorite tracks, and album, playlist and artist pages reached from them, with spotify-player's navigation (a page history, focusable windows, an actions popup). From any track list a track plays **with the rest of its list** queued after it, or is added to the queue. It also gives the queue its long-promised remove key (0004 "Out of scope").

It settles 0005's open question (0005 decision 3, "Out of scope"): **the player fetches the pages**, as it already expands `Open`. A client still holds no session, no keyring entry and no API access (0005 AC14 keeps holding).

Search (0007), mixes and radio as pages (0011), adding or removing favorites and editing playlists (decision 4), and caching (0009) are not in this spec.

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

1. **Playing from a list plays one track** (tidalt `59c610d`, #5) → `Enter` on a track row sends the **whole list** with the chosen index, as one `LoadQueue` (AC10)
2. **Lists sent in a display order** (tidalt #21) → a page's list is sent in the order Tidal returned it; the client never sorts or shuffles it (AC10)
3. **Videos queued as tracks** → favorites, playlist and artist lists keep only tracks; video items are dropped where the API mixes them in (AC3–AC6)
4. **A list cut at one page** (tidalt: 50 favorite tracks, 50 playlists, 200 albums and artists; Tidal's default page is 10 items on several endpoints, 0004 "Facts") → every list is walked to its end, and a list above the cap says so in its title (AC3–AC6)
5. **Followed playlists missing** (tidalt listed only its own) → the library lists own and favorite playlists (AC3)
6. **A page that draws another list** (tidalt `65285f1`: Favorite Songs drew the queue) → each page renders its own fetched data only (AC14, a snapshot per page)
7. **Stale or zero counts** (tidalt `64d3695`) → counts come with each fetch, and opening a page always fetches it fresh (AC8)
8. **Browsing that stalls playback**: a fetch of thousands of favorites must never delay `Next` → fetches run beside the player's input loop, never in it (AC7)
9. **A result landing on the wrong page**: the user moved on while a fetch was in flight → every fetch carries an ID, and a result is applied only to the page that asked for it (AC8)
10. **A client that needs its own login**, as tidalt's did (0005 "What went wrong" 9) → pages come through the player (AC7, AC16)
11. **An album opened from an artist plays at once** (tidalt `3b6d6ea`) → `Enter` on an album opens its page; only `Enter` on a track plays (AC10)
12. **A short page taken for overlap or the end** (the probe: 358 favorite tracks on a 1000-item page for a total of 362) → `offset` advances by the page size, the walk ends on the total, duplicates are dropped (AC3, AC4)

## Behaviour

### Pages

The area below the playback window shows one **page** at a time. The queue (0004) is the first page. Opening a page puts it on top of a **history**; going back returns to the page under it, with its data, cursors and focus as they were, without fetching again. Opening a page always fetches it fresh (nothing is cached until 0009).

| Page | Opened with | Windows (`Tab` moves between them) |
|---|---|---|
| Queue | `z`; it is the page at start | the queue (0004) |
| Library | `g l` | **Playlists**, **Albums**, **Artists** |
| Favorite tracks | `g y` | the tracks |
| Album | `Enter` on an album; *Go to album* | the album's tracks |
| Playlist | `Enter` on a playlist | the playlist's tracks |
| Artist | `Enter` on an artist; *Go to artist* | **Top tracks**, **Albums** |

- Opening the page that is already on top does nothing. Otherwise the new page is pushed; the history keeps at most **50** pages (the oldest is dropped; the queue at the bottom is never dropped)
- `Backspace` (and `C-q`) goes back; on the bottom page it does nothing
- `z`, `g l`, `g y` open their page from anywhere (pushed like any other)
- Each page has a **title row** (the window titles carry the counts):
  - Library: `Library`
  - Favorite tracks: `Favorite tracks · 1234 tracks`
  - Album: `<title> · <artists> · <year> · 17 tracks · 1:02:15` (year and length when known)
  - Playlist: `<title> · 39 tracks · 2:41:07`
  - Artist: `<name>`
- A page being fetched shows `Loading…` in its windows; a failed fetch shows the message in the page (`Could not load the library: <reason>`, `Album 1 was not found`, …); an empty list shows `No favorite tracks yet`, `No playlists yet`, `No favorite albums yet`, `No favorite artists yet`, `This playlist has no tracks`
- A list longer than **10 000** items keeps its first 10 000 and the window title says `(first 10000 of 12345)`

### Lists and the cursor

Every window is a list with its own cursor, moved by 0004's keys (`j`/`↓`, `k`/`↑`, `g g`, `G`), plus `C-f`/`PageDown` and `C-b`/`PageUp` (a window's height). The focused window's cursor is highlighted; the others' are drawn dim. `Tab` focuses the next window, `BackTab` (Shift-Tab) the previous, wrapping. Cursors are by position on browse pages (their lists never change after the fetch) and by entry ID on the queue (0004 AC22, unchanged).

Rows:

- **Track rows** (favorite tracks, album, playlist, top tracks, queue): 0004's queue columns (number, title, artists, album, length). Tracks that are not streamable are drawn dim, as the player will skip them (0004 AC7)
- **Album rows**: title, artists, year
- **Playlist rows**: title, number of tracks
- **Artist rows**: name

### Playing and queueing from a page

| Key | On | Does |
|---|---|---|
| `Enter` | a track (browse page) | Replaces the queue with **every track of that list**, in page order, and plays the chosen one: `LoadQueue { tracks, start: index }` (what went wrong 1, 2) |
| `Enter` | a track (queue page) | Plays that entry (0004, unchanged) |
| `Enter` | an album, playlist or artist | Opens its page |
| `Z`, `C-z` | a track | Adds it at the end of the queue: `AddToQueue { tracks: [track], at: End }` |
| `Z`, `C-z` | an album or playlist | Adds all its tracks at the end of the queue: `Open { items: [album / playlist], at: Some(End) }` (the player expands it, 0005) |
| `Z`, `C-z` | an artist | Nothing |
| `d` | an entry (queue page) | Removes it: `RemoveFromQueue(entry ID)` (0004's rules: removing the current entry moves on) |

With nothing playing (an empty queue, or stopped with nothing current), `Z` on an album or playlist starts the first added track (0005's `Open` rule). For `Z` on a track, see decision 3.

### Actions popup

spotify-player's actions popup: `g a` or `C-Space` on the selected row, `a` on the playing track. A small list over the page, titled with the item; `j`/`k` move, `Enter` runs the action and closes the popup, `Esc` closes it. While it is open no other key acts.

| Item | Actions, in this order |
|---|---|
| Track (browse page) | *Go to album*, *Go to artist* (one per artist: `Go to artist: <name>`), *Add to queue*, *Play next* |
| Entry (queue page), or the playing track (`a`) | *Go to album*, *Go to artist …*, *Play next* (not for the playing track), *Remove from queue* |
| Album | *Open*, *Go to artist …*, *Add to queue*, *Play next* |
| Playlist | *Open*, *Add to queue*, *Play next* |
| Artist | *Open* |

*Play next* is *Add to queue* with `at: Next`. *Go to album* is missing for a track without an album; `a` with nothing playing does nothing.

### `Esc` and quitting

As in spotify-player: `Esc` closes the popup or the open prompt, and otherwise does nothing; `q` (and `C-c`) quits. This changes 0004, where `Esc` also quit (decision 2; 0004 AC20's row for `Esc` is updated).

### Fetching through the player

A client asks the player for a page with a new message, answered to that client alone:

- `ClientMessage::Fetch { id, request }` → `ServerMessage::Fetched { id, result: Ok(page) | Err(message) }`
- `request` is one of `Library`, `FavoriteTracks`, `Album(id)`, `Playlist(uuid)`, `Artist(id)`; `page` carries the matching data (types below)

Rules:

- Fetches run as jobs beside the player's input loop (as 0004's suggestions and 0005's `Open` expansions do), so a fetch never delays a command or an event. A client's fetches run one at a time, in the order sent; different clients' run independently
- `Fetched` is sent once per `Fetch`, also when it failed. It is not an `Event`: other clients do not see it
- A client that disconnects loses its pending fetches' answers (it asks again after reconnecting, below)
- The message on failure is the metadata error's (0004's wording): `Album 1 was not found`, `Playlist <uuid> was not found`, `Artist 1 was not found`, the session-expired message for `LoginRequired`, and `Could not reach Tidal: …`/`Tidal answered 429: try again in a moment` for transient errors. A fetch is never retried in a loop: the user opens the page again

The client side ([client model](#client-model)) tags each fetch with a fresh ID; a `Fetched` whose ID belongs to no page in the history is dropped (what went wrong 9).

### What the player fetches

All requests go through the `Authenticator` with `countryCode` from the session (0004), `{user}` being the session's `user_id`. Endpoints, envelopes, page sizes and orders are the library probe's ("Facts vs. assumptions").

**Walking a list** (the endpoints of this spec; 0004's album and playlist walks are unchanged): pages of the endpoint's page size *P* (table), `offset` advanced by ***P*, not by the items received**, until `offset ≥ totalNumberOfItems` or the 10 000-item cap; items already seen (same ID or UUID) are dropped. The probe found a page shorter than asked without being the end: favorite tracks with `limit=1000` returned 358 items for a `totalNumberOfItems` of 362 (the server leaves out what it will not serve, after paging), so advancing by the items received would ask for overlapping pages. The counts shown are the items kept, not `totalNumberOfItems`; the cap's title uses `totalNumberOfItems`.

| Request | API calls | Kept |
|---|---|---|
| `Library` | `GET /users/{user}/playlistsAndFavoritePlaylists?order=DATE&orderDirection=DESC` (*P* = 50), `GET /users/{user}/favorites/albums?order=DATE&orderDirection=DESC` (*P* = 1000), `GET /users/{user}/favorites/artists?order=DATE&orderDirection=DESC` (*P* = 1000); the three in parallel | playlists from `items[].playlist` (UUID, title, `numberOfTracks`, `duration`; **own** when the entry's `type` is `USER_CREATED`, favorite when `USER_FAVORITE`), albums from `items[].item` (ID, title, artists, year from `releaseDate`, `numberOfTracks`, `duration`), artists from `items[].item` (ID, name); each newest first (by the entry's `created`, the date it was added) |
| `FavoriteTracks` | `GET /users/{user}/favorites/tracks?order=DATE&orderDirection=DESC` (*P* = 1000) | tracks from `items[].item` (0004's mapping), newest first |
| `Album(id)` | `GET /albums/{id}` and `GET /albums/{id}/tracks` (0004), in parallel | header (title, artists, year from `releaseDate`, `numberOfTracks`, `duration`) and every track |
| `Playlist(uuid)` | `GET /playlists/{uuid}` and `GET /playlists/{uuid}/items` (0004, `type == "track"` only), in parallel | header (title, `numberOfTracks`, `duration`) and every track |
| `Artist(id)` | `GET /artists/{id}`, `GET /artists/{id}/toptracks?limit=100` (one request), `GET /artists/{id}/albums` and `GET /artists/{id}/albums?filter=EPSANDSINGLES` (*P* = 1000 each), in parallel | name; top tracks (bare tracks, in the API's order, at most 100); albums (the plain call gives only `type: ALBUM`), then EPs and singles (`EP`/`SINGLE`), each in the API's order |

A `404`/`2001` on the item's own resource (`/albums/{id}`, `/playlists/{uuid}`, `/artists/{id}`) is `NotFound`; an unknown artist's `/albums` is a `200` with no items, so only `/artists/{id}` says it does not exist. Any failing call fails the whole fetch with that call's error (no half pages). Without `order`/`orderDirection` the favorites lists come in no date order (the probe's default page mixed 2024 and 2025 entries), so the parameters are always sent.

### Types (`tidal_player_core`)

- `Track` gains the IDs needed to go to its album and artists: `artists: Vec<ArtistRef { id, name }>` (was names only) and `album: Option<AlbumRef { id, title }>` (was the title only). The mapping fills them from the existing `artists[].id`/`album.id` fields (0004 "Facts": the track DTO has both). Rendering is unchanged
- `library` module: `AlbumSummary { id, title, artists: Vec<ArtistRef>, year: Option<u16>, tracks: Option<u32>, duration: Option<Duration> }`, `PlaylistSummary { uuid, title, tracks: Option<u32>, duration: Option<Duration>, own: bool }`, `ArtistRef`, `FetchRequest`, and `FetchedPage`: `Library { playlists, albums, artists }`, `Tracks { title_row, tracks }` (favorite tracks), `Album { album, tracks }`, `Playlist { playlist, tracks }`, `Artist { artist, top_tracks, albums }`. Each list carries `total` (the API's `totalNumberOfItems`) besides its kept items, for the cap's title
- All of them `Serialize`/`Deserialize`, as they cross the socket. The 16 MiB line limit (0005) holds 10 000 tracks with room to spare (about 300 bytes each)

### Client model

`tidal_player_core::ui::State` gains the history (`Vec<Page>`, bottom first), each `Page` holding its kind, its fetch (`Loading { id }`, `Loaded(FetchedPage)`, `Failed(message)`), its windows' cursors and its focus; and the popup. `Effect` gains `Fetch { id, request }`; `Action` gains `Fetched { id, result }`. Fetch IDs come from a counter in the state (unique per client run).

- While **disconnected** (0005), loaded pages can still be browsed and opened from history, but nothing is fetched or sent: opening a new page shows it `Failed` with the disconnected message, and pages still `Loading` fail with it. On the next `Welcome`, the page on top, if it failed that way, is fetched again
- When the player says the **session expired**, fetches fail with the session-expired message; after `LoginRestored`, opening the page again works

### Rendering

The page below the playback window replaces the queue's area (0004 "TUI" rules hold: the playback window keeps its 4 rows, nothing shares rows with it). A page with several windows draws them side by side with these widths, when the inner width is at least 60 columns:

- Library: Playlists 40 %, Albums 40 %, Artists 20 % (spotify-player's defaults)
- Artist: Top tracks 60 %, Albums 40 %

Below 60 columns only the focused window is drawn, its title followed by `‹Tab›`. The title row takes one row above the windows. The popup is centred over the page, at most 50 columns wide and as tall as its actions, clipped to the page. Rows are truncated with `…` as in 0004; nothing panics at any size (0 × 0 included).

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Library                                                                       │
│┌Playlists (12)──────────────┐┌Albums (48)─────────────────┐┌Artists (31)────┐│
││Running                  42 ││Collide With The Sky  Pier…  ││Pierce The Veil ││
││Late night               17 ││Misadventures         Pier…  ││Sleeping With S…││
│…                                                                             │
```

## Acceptance criteria

Protocol and types (`tidal_player_core`, pure):

- **AC1** — `ClientMessage::Fetch`, `ServerMessage::Fetched`, every `FetchRequest` and `FetchedPage` variant, `AlbumSummary`, `PlaylistSummary`, `ArtistRef`/`AlbumRef` and the new `Track` shape survive a JSON round trip (0001 AC11's test, extended); 0005's codec decodes both new messages
- **AC2** — `Track`'s new fields: the 0004 metadata mapping fills `artists[].id` and `album.id` from the existing fixtures; a track with `album: null` maps to `album: None`. Every place that showed artist names and the album title shows the same text as before (0004/0005 rendering snapshots unchanged)

API (`tidal-player-api::library`, wiremock fixtures written from the probe):

- **AC3** — `get_library()`: the three lists' requests (paths with the session's user ID, `countryCode`, `order=DATE&orderDirection=DESC`, `limit` = *P*, `expect(n)` per page), each walked to its end with `offset` advanced by *P* (a 120-playlist fixture: 3 requests at offsets 0, 50, 100); a page shorter than *P* before the total does not end the walk; duplicates across pages dropped; mapped summaries in the API's order; `own` from `USER_CREATED`/`USER_FAVORITE`; the 10 000 cap with `total` kept (10 050 favorite albums: 10 000 kept, no request at offset 10 000)
- **AC4** — `get_favorite_tracks()`: the order parameters and *P* = 1000 sent, every page (2 500 tracks: 3 requests), `item` unwrapped and mapped as 0004's tracks, a 1000-item page that returns 996 items followed by the next page at offset 1000 (the probe's 358-of-362 case), the cap
- **AC5** — `get_album(id)` and `get_playlist(uuid)`: header and tracks; the tracks are 0004's walks (playlist videos dropped); `404`/`2001` on the header → `NotFound`; a failing tracks call fails the whole fetch
- **AC6** — `get_artist(id)`: name, top tracks (exactly one request, `limit=100`), albums then EPs and singles (`filter=EPSANDSINGLES`); an unknown artist whose `/albums` answers `200` with no items still gives `NotFound` from `/artists/{id}`; `404`/`2001` → `MetadataError::NotFound(Artist)` with the message `Artist 1 was not found`; `LoginRequired` and transient errors returned unchanged by every call (table over the AC3–AC6 calls)

Player (`tidal-player`, fake library source, 0005's server tests):

- **AC7** — `Fetch` is answered with exactly one `Fetched` with the same `id`, to that client only (another subscriber receives nothing); errors become `Err(message)` with the messages under "Fetching through the player" (table: not found, login required, network, 429); while a fetch is held pending by the fake, the same client's `Request { TogglePause }` is replied within 1 s and events keep flowing; one client's second fetch starts after its first answered, while another client's fetch proceeds independently; a client that disconnects mid-fetch leaves the player unaffected

Client model (`tidal_player_core::ui`, pure):

- **AC8** — History: `z`, `g l`, `g y` and `Enter` on album/playlist/artist rows push the page and emit `Fetch` with a fresh ID (the queue page emits none); opening the top page again does nothing; `Backspace`/`C-q` pop and the page under shows its kept data, cursors and focus with no `Fetch`; the bottom page is never popped; the 51st push drops the oldest page above the queue. `Fetched` applies to the page with that ID only, also when it is not on top; one for an ID no page has is dropped (table)
- **AC9** — Windows and cursors: `Tab`/`BackTab` cycle the focus over the page's windows, wrapping; `j`/`k`/`g g`/`G`/`C-f`/`C-b` move the focused window's cursor only, clamped; cursors survive leaving and coming back (history); a `Loading`/`Failed`/empty window ignores cursor keys
- **AC10** — Playing and queueing (table over every row of "Playing and queueing from a page"): `Enter` on track *i* of an *n*-track list sends `LoadQueue { tracks: the list in page order, start: i }` (the same `Vec` as the page's, never sorted or shuffled); `Z`/`C-z` on a track sends `AddToQueue { [track], End }`; on an album/playlist `Open { [item], Some(End) }`; on an artist nothing; `d` on the queue sends `RemoveFromQueue(entry ID)` and on a browse page nothing; the queue's `Enter` is 0004's
- **AC11** — Actions popup: `g a`/`C-Space` open it for the selected row and `a` for the playing track, with the actions per item kind and order of the table (table, incl. a track with two artists and one without an album; `a` with nothing playing opens nothing); `j`/`k` move, `Enter` emits the action's effect (`Go to …` pushes and fetches that page; *Play next* sends `at: Next`) and closes it; `Esc` closes it with no effect; while open, no other key acts (`Space`, `n`, `q` included)
- **AC12** — `Esc` with no popup or prompt open emits nothing (0004 AC20's `Esc` row changed from `Quit`); `q` and `C-c` emit `Quit`
- **AC13** — Disconnected and login (table): while disconnected, opening a page shows it failed with the disconnected message and emits no `Fetch`, a `Loading` page fails with it, history and cursors work, `Enter`/`Z`/`d` emit nothing; the first `Welcome` after that re-fetches the top page if it failed that way (and only it); a `Fetched` error with the session-expired message is shown in the page

TUI rendering and runtime (`tidal-player`):

- **AC14** — Rendering (`insta`, 80×24, reviewed by eye, plus `contains` checks): library loaded (three windows, counts, focus highlight); library loading; favorite tracks with a non-streamable row dimmed; album page title row; artist page (two windows); a failed page; each empty-list message; the `(first 10000 of 12345)` title; the actions popup over a track list. 50×20: only the focused window with `‹Tab›`. No panic from 0×0 to 120×40 on every page (table over sizes × pages)
- **AC15** — Key decoding: `Tab`, `BackTab`, `Backspace`, `PageUp`/`PageDown`, `C-Space`, `C-f`/`C-b`/`C-q`/`C-z`/`C-c` decode to the model's keys (`crates/app/src/input.rs`, table)
- **AC16** — Wiring: the TUI client runtime sends `Effect::Fetch` as `ClientMessage::Fetch` and turns `Fetched` into `Action::Fetched`, both attached and standalone (in-process link, 0005); an attached client fetching a page still opens no session store, keyring or passphrase source (0005 AC14's test, with a `g l` fetch answered from the test player's wiremock API)

Docs:

- **AC17** — `docs/tui.md` documents the pages, the history, the windows, playing and queueing from a page, the actions popup, the new keys and the changed `Esc`, and the empty/failure messages; `CLAUDE.md` "Status" names this spec; this spec links to it

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| Thousands of favorite tracks | Walked page by page in the player while the TUI shows `Loading…`; playback and other keys keep working (AC7). Above 10 000, the first 10 000 (AC3, AC4) |
| `Enter` on a track of a 10 000-track list | One `LoadQueue` of 10 000 tracks (about 3 MB on the socket, under the 16 MiB limit) |
| The user opens pages faster than they load | Each page has its own fetch ID; results land on their own page or are dropped (AC8); the client's fetches run one at a time in the player (AC7), so mashing does not burst requests at Tidal |
| `429` while walking a list | The fetch fails with the message; nothing retries in a loop (0004 "What went wrong" 3) |
| An album or playlist removed from Tidal but still in favorites | Shown in the list as Tidal returns it; opening it shows `Album 1 was not found` |
| A track unavailable in the user's country | Shown dimmed; queued, then skipped by the player as a track-only failure (0004 AC7) |
| A playlist of only videos | `This playlist has no tracks`; `Z` on it in the library: `Playlist <uuid> has no tracks` (0004) |
| An artist with no albums or no top tracks | That window says `No albums` / `No top tracks` |
| A track without an album (`album: null`) | No *Go to album* action |
| Session expires while browsing | Fetches fail with the session-expired message; the status line shows it (0002 AC14) |
| The player shuts down or the connection drops while a page loads | The page fails with the disconnected message and is fetched again on reconnect if still on top (AC13) |
| Version mismatch between client and player | Refused at the greeting (0005), as before: the new messages are never seen by an old player |
| Narrow terminal | One window at a time (AC14); below 6 inner rows only the playback window (0004) |

## Test plan

Each automated test is named after its criterion (`ac8_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0005. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/library/`, written from the probe's recorded shapes (IDs and names replaced by fakes); long lists are built by the tests from a one-item fixture. Server tests reuse 0005's harness with a fake library source that can hold a fetch pending.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (extended) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | new types' hand-written stub `Serialize` writes `null` |
| AC2 | `crates/api/tests/metadata.rs` :: `ac2_track_refs` | IDs mapped; `album: null` → `None` | stub mapping leaves `id` 0 |
| AC3 | `crates/api/tests/library.rs` :: `ac3_library` (+ `ac3_cap`) | paths, params, page counts, mapping, cap | stub reads the first page only |
| AC4 | `crates/api/tests/library.rs` :: `ac4_favorite_tracks` | order params, pages, unwrapping, videos dropped | stub drops nothing and reads one page |
| AC5 | `crates/api/tests/library.rs` :: `ac5_album_and_playlist` | header + tracks; errors | stub returns an empty header |
| AC6 | `crates/api/tests/library.rs` :: `ac6_artist`, `ac6_errors` (table) | the four calls; errors per call | stub skips EPs and singles and maps `404` to a transient error |
| AC7 | `crates/app/src/ipc/server/tests.rs` :: `ac7_fetch_replied_to_sender`, `ac7_fetch_errors` (table), `ac7_fetch_does_not_block` | one reply, sender only, messages, latency, per-client order | stub runs the fetch on the player thread, so the held fetch blocks the `TogglePause` reply |
| AC8 | `crates/core/src/ui.rs` :: `ac8_history` (table), `ac8_fetched_by_id` (table) | stack, effects, kept state, stale drop | stub replaces the page instead of pushing, so back does nothing |
| AC9 | `crates/core/src/ui.rs` :: `ac9_windows_and_cursors` (table) | focus cycle, cursor per window, clamping | stub moves every window's cursor |
| AC10 | `crates/core/src/ui.rs` :: `ac10_play_and_queue` (table) | the exact command per row | stub `Enter` sends `LoadQueue` of the chosen track alone (tidalt `59c610d`) |
| AC11 | `crates/core/src/ui.rs` :: `ac11_actions_popup` (table) | actions per kind, effects, closing, key capture | stub popup lists *Add to queue* only |
| AC12 | `crates/core/src/ui.rs` :: `ac12_esc_and_quit` + 0004's `ac20_keys_to_commands` (row updated) | `Esc` → nothing; `q`/`C-c` → `Quit` | stub keeps `Esc` → `Quit` |
| AC13 | `crates/core/src/ui.rs` :: `ac13_disconnected_and_login` (table) | no `Fetch`/`Send` while disconnected; one re-fetch | stub fetches while disconnected |
| AC14 | `crates/app/src/ui.rs` :: `ac14_pages_80x24` (one snapshot per row), `ac14_narrow_50x20`, `ac14_no_panic_any_size` | `contains` checks + snapshots | stub render draws the queue whatever the page |
| AC15 | `crates/app/src/input.rs` :: `ac15_key_events` (table) | decoded keys | stub maps the new keys to nothing |
| AC16 | `crates/app/src/client.rs` :: `ac16_fetch_round_trip` (attached and in-process) + `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (extended with a library fetch) | messages both ways; no session opened | stub client drops `Effect::Fetch` |
| AC17 | — reviewed at acceptance | docs match this spec, linked | — |

Not covered by automated tests, on purpose, and checked by hand at acceptance with a real account (results in the PR description):

- `g l` on the user's account lists the same playlists, albums and artists (and counts) as the Tidal app; `g y` the same favorite tracks, in the same order
- A large favorites list loads while a track plays, without a hiccup in playback or the keys
- `Enter` in a playlist plays it from that track with the rest queued; `Z` on an album in the library adds it; *Go to artist* from the playing track opens the artist

## Crate placement

- `tidal-player-core`: `library` (summaries, refs, `FetchRequest`, `FetchedPage`), `track` (`ArtistRef`, `AlbumRef`), `protocol` (`Fetch`, `Fetched`), `ui` (pages, history, windows, popup, `Effect::Fetch`, `Action::Fetched`, the new keys). No I/O, no new dependency
- `tidal-player-api::library`: the fetches and their DTO → core mapping; `metadata`'s walk is shared (moved to a common helper, page size per endpoint), and `MetadataError::NotFound` takes the new `Artist` kind
- `tidal-player`: `ipc::server` (`Fetch` → a job; `Fetched` to that client's outbox), `player_runtime` (a `Library` trait beside 0004's `Metadata`, implemented by the API client and faked in tests), `client.rs` (the effect and the action), `ui.rs` (pages, windows, popup rendering), `input.rs` (new keys)
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

Not verified:

- Whether the short page (358 of 362) is the server dropping tracks unavailable in the country, and whether it happens on pages other than the last (the walk handles both, AC4)
- Page sizes above 1000 on the favorites albums/artists and artist-albums endpoints (not needed: the walk uses 1000)
- The order the Tidal apps themselves show the library in
- Whether 0004's album and playlist walks (`offset` advanced by the items received) meet the same short pages on `/albums/{id}/tracks` or `/playlists/{uuid}/items`. If they do, they would request overlapping pages and queue a track twice; this spec leaves them as they are, and a 0004 "Bugs" entry follows if it is seen

Verified from tidalt's code and history (2026-10-07): the list under "What tidalt did". Not verified: whether tidalt's single-request lists were actually cut short on a real account (inferred from the `limit` values; no issue reports one)

## Decisions (answered by the user, 2026-10-07; the body is updated to them when work resumes)

1. **Where pages are fetched**: *in the player* (`Fetch`/`Fetched`), as proposed
2. **`Esc`**: *as proposed*: closes the popup or the prompt only; `q`/`C-c` quit (0004 AC20's `Esc` row changes)
3. **`Z` on a track with nothing playing**: *start it*, sent as `Open { items: [Item::Track(id)], at: Some(End) }`
4. **Favoriting and playlist editing**: *in this spec* (2026-10-07: "why not", then "yes" to playlist editing). Actions popup: *Add to favorites*/*Remove from favorites* for tracks, albums, artists and playlists; *Add to playlist…* (pick one of your own playlists, or a new one), *Remove from playlist* on a track of your own playlist, *New playlist*, *Delete playlist* (own). Shapes, ETags and error codes come from `scripts/tidal-library-write-probe.sh`, to be run before approval
5. **Lists load as you scroll** (2026-10-07: "can do inf-fetching if they do"): Tidal pages with `limit`/`offset` (= page size/page index × size) and returns `totalNumberOfItems` (the read probe). So a page fetches its first page of *N* items (the **page size**, a setting, default **100**, 1–10 000, clamped per endpoint: 50 for playlists) and shows the total at once (`Favorite tracks · 362 tracks`); the next page is fetched when the cursor comes within one window height of the last loaded row (`Loading more…` as the last row). Playing or queueing a whole list (`Enter` on a track, `Z`/*Add to playlist* on a list) first fetches the rest, then sends it. No 10 000-item cap on lists any more; `Enter` on a list longer than the socket's 16 MiB line can hold (about 50 000 tracks) is refused with a message
6. **Artist page**: windows **Top tracks**, **Albums** (albums, then EPs and singles), **Appears on** (`filter=COMPILATIONS`), and **All tracks**. The user asked (2026-10-07) whether *All tracks* is what the Tidal apps show as "Credits for <artist>"; `scripts/tidal-artist-tracks-probe.sh` (read-only) looks for an endpoint behind it (v1 `credits`/`contributions`/`tracks`, the contributor and artist pages, v2 `artists/{id}/relationships/tracks`). If one exists, *All tracks* uses it, paged as decision 5; note that "Credits" may include songwriting and production credits on other artists' tracks. Otherwise *All tracks* is built: every track of every album, EP and single, then the tracks of the *Appears on* albums that credit this artist (by artist ID), without duplicates (same track ID), one request per release, loaded when the window is first focused (`Loading… (12 of 108 releases)`).

   **Alternate versions are left out of *All tracks*** (2026-10-07: instrumental, then "TV Version, Speed Up, Slowed + Reverb, etc."). A track is left out when its `version`, or a bracketed part or ` - ` suffix of its title, matches one of these after lower-casing and removing spaces, hyphens, underscores, dots and `+`: `instrumental`, `inst`, `offvocal`, `karaoke`, `tvversion`, `tvver`, `tvsize`, `tvedit`, `spedup`, `speedup`, `speedupversion`, `nightcore`, `slowed`, `slowedreverb`, `slowedandreverb`, `reverb`, `8d`, `8daudio`. Table-tested (e.g. `TV-size`, `TV Size ver.`, `(Sped Up)`, `Slowed + Reverb`, `[Instrumental]`, `- Off Vocal`), and the list becomes a setting with 0008. Other windows (albums, top tracks, playlists, favorites) show every version

## Out of scope

- Search (0007)
- Mixes, radio and "My mixes" pages (0011); spotify-player's *GoToRadio*
- Adding and removing favorites, creating and editing playlists (decision 4)
- Caching pages or images, remembering the page history across runs (0009)
- Cover art on pages (0004's placement rule applies when it comes)
- Sorting and filtering lists (spotify-player's `s t`, `/` in a page), the help popup (`?`), configurable keys and page percentages (0008)
- Jumping to the playing track in its list (spotify-player's `g c`), the "currently playing context" page (`g space`)
- One-shot `playback` commands for the library (e.g. `playback load --favorites`)
