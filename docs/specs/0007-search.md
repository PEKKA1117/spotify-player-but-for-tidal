# 0007 — Search

- **Status**: draft (2026-10-08); decisions answered by the user (see "Decisions"); API shapes verified by the live probe (2026-10-08, "Facts"); waiting for approval
- **Owner**: tech-lead (primary session)
- **Depends on**: 0006 (implemented: pages, the history, windows that load as you scroll, `Enter`/`Z`/actions on rows, `Library`/`LibraryReply`)
- **User docs**: [`docs/tui.md`](../tui.md) gains a "Search" section and the new key (AC14)

## Context

0006 lets the user browse what is already theirs. To play anything else they still paste a link or an ID (0004 `o`/`O`). This spec adds spotify-player's **search page**: type a query, get Tidal's top hit and its matching tracks, albums, artists and playlists in four windows, and use them exactly as rows of any other page (play, queue, open, actions popup).

It adds no new mechanism where 0006 has one: the search page is one more page in the history, fetched through the player (`PageRequest::Search`), its four lists load as you scroll (`ListRef::Search*`), and its rows are 0006's track, album, artist and playlist rows with 0006's keys and actions.

### What tidalt did

Read from tidalt's `internal/tidal/library.go` (`SearchAll`), `internal/tidal/api.go` (`Search`) and `internal/ui/search.go` (`6876b42`, `1dc0f4b`):

- **One request, 20 per type, no paging**: `GET /v1/search?query=…&types=TRACKS,ARTISTS,ALBUMS,PLAYLISTS&limit=20&countryCode=…`. Whatever came after the 20th result was out of reach, with nothing saying so
- **Playlists fetched but never shown**: the response's playlists were decoded, but the flattened row list (`searchRows`) held only tracks, artists and albums
- **One flattened list** (tracks, then artists, then albums) under the input, the cursor walking from the input into it with `↓`
- **`Enter` on a track played that track alone** (`playTrackCmd`), so playback stopped after it (0004 "What went wrong" 9)
- **`Enter` on an album loaded it into the queue** (`openAlbum`), with no way to see its tracks first (0006 "What tidalt did", `3b6d6ea`)
- Results replaced on each submit with no request ID: a slow earlier search could land after a later one

### What could go wrong here, and the criterion that covers it

1. **Results cut at one page** (tidalt: 20 per type) → each window loads more as you scroll and its title shows Tidal's total (AC3, AC8)
2. **A result type fetched but not shown** (tidalt: playlists) → four windows, each with a rendering snapshot (AC12)
3. **An old search landing over a newer one** → each search is a request with its own ID; only the page's latest one is applied (AC7)
4. **Typing that triggers playback keys**: `q`, `n`, `Space` typed into the query must not quit, skip or pause → while the input has the focus every printable key types (AC6)
5. **A search per keystroke**: requests at Tidal's rate limit (0004 "What went wrong" 3) → a search is sent on `Enter` only (decision 2) (AC7)
6. **`Enter` on a track plays one track** (tidalt) → the track plays with the track results queued after it (decision 4) (AC9)
7. **`Enter` on an album queues it** (tidalt) → it opens the album page, as everywhere (AC9)
8. **Unplayable or non-music results**: videos come back unless `types` is sent, and track results include Dolby-Atmos-only copies (the probe: a `LOW` `DOLBY_ATMOS` "Hell Above") → `types` is always sent, and tracks without `STEREO` in `audioModes` are dropped, as 0006's *All tracks* does (AC2, AC3)

## Behaviour

### The search page

`g s` opens the **search page** (spotify-player's `SearchPage`), pushed on the history like any 0006 page; if the page on top is already a search page, `g s` gives its input the focus instead. The page has a one-row **input**, a one-row **Top hit** and four windows below them:

| Window | Rows (0006's) | Title |
|---|---|---|
| **Top hit** | one row of any kind, with its kind (`Pierce The Veil · artist`); absent when Tidal names none (decision 6) | — |
| **Tracks** | track rows | `Tracks (1 234)` |
| **Albums** | album rows | `Albums (87)` |
| **Artists** | artist rows | `Artists (12)` |
| **Playlists** | playlist rows | `Playlists (300)` |

- The page opens with an empty input that has the **focus**, and the windows show nothing. The title row reads `Search`; after a search, `Search · "<query>"`
- **Input keys** (while the input has the focus): printable characters (`Space` included) and pasted text are appended; `Backspace` deletes the last character; `C-u` clears the input; `Enter` sends the search; `Tab`/`BackTab` move the focus to the windows; `Esc` gives the focus to the window a search would focus (below) if there are results, else does nothing. Every other key is ignored, so `q`, `n`, `Space`, `g …` and `Backspace` on an empty input do not act (`C-c` still quits, `C-q` still goes back)
- **Sending**: `Enter` with an input that is empty after trimming spaces does nothing. Otherwise the trimmed query (at most 200 characters, longer input is not accepted: typing stops at 200) is sent as `Library { Page(Search(query)) }`; all four windows show `Loading…`, then the first page of each list, and the focus moves to the *Top hit* when there is one, else to the first non-empty window of *Tracks*, *Albums*, *Artists*, *Playlists*, else stays on the input. A new search on the same page replaces the results; cursors return to the top
- **Windows**: `Tab`/`BackTab` cycle the focus over input → *Top hit* (when shown) → *Tracks* → *Albums* → *Artists* → *Playlists* → input, wrapping (spotify-player's order, with the top hit first as in the Tidal apps). With the focus on a window, every 0006 key works as on any page: cursor keys, `Enter`, `Z`, `g a`/`C-Space`, `a`, `Backspace` (back), `g l`, `z`, … `/` gives the input the focus again (spotify-player uses `/` for its in-page search popup, 0008; on this page it means "search again")
- **History**: going back to a search page shows its query, results, cursors and focus as they were, without searching again. `g s` from another page pushes a new, empty search page (the earlier one stays in the history below)
- **Empty results**: a window with no results says `No tracks found`, `No albums found`, `No artists found`, `No playlists found`; a failed search shows its message in the page (`Could not search: <reason>`), as a failed 0006 page does

### Lists load as you scroll

Each window is a 0006 list: the first page comes with the search (page size `TIDAL_PLAYER_PAGE_SIZE`, clamped per endpoint), the next when the cursor nears the last loaded row, `Loading more…`, failures as the last row, rows already loaded skipped by ID. Tidal returns at most about 300 results per list (the probe: totals of 295, 300, 189, 125; an `offset` past the total answers `200` with no items), so a list holds at most three pages at the default page size.

### Playing and queueing from results

0006's table holds, with one difference for `Enter` on a track (decision 4), and the top hit acts as a row of its kind:

| Key | On | Does |
|---|---|---|
| `Enter` | a track | Replaces the queue with the **loaded** rows of *Tracks*, in result order, and plays the chosen one: `LoadQueue { tracks, start: index }`. Nothing more is fetched first (a search for "love" can match thousands of tracks) |
| `Enter` | the top hit, a track | As `Enter` on that track in *Tracks* when it is among the loaded rows; otherwise `LoadQueue { [top hit] + the loaded rows of Tracks, start: 0 }` |
| `Enter` | an album, playlist or artist (top hit or row) | Opens its page (0006) |
| `Z`, `C-z` | a track, album or playlist | Adds it at the end of the queue (0006) |
| `g a`, `C-Space` | any row | 0006's actions popup for that kind of row (a search playlist is never *own*: no *Delete playlist*, and *Add to favorites*/*Remove from favorites* are both shown) |

### Talking to the player

0006's `Library` request, two additions:

- `PageRequest::Search(String)` → `PageData::Search { top_hit: Option<TopHit>, tracks: ListPage<Track>, albums: ListPage<AlbumSummary>, artists: ListPage<ArtistRef>, playlists: ListPage<PlaylistSummary> }`
- `TopHit` = `Track(Track) | Album(AlbumSummary) | Artist(ArtistRef) | Playlist(PlaylistSummary)`
- `ListRef::SearchTracks(String)`, `SearchAlbums(String)`, `SearchArtists(String)`, `SearchPlaylists(String)` for `More`

Errors are 0006's (`Could not reach Tidal: …`, `Tidal answered 429: try again in a moment`, the session-expired message); a request Tidal refuses as invalid (`400`) shows `Tidal refused the search: <userMessage>`.

### What the player fetches

Shapes from the probe ("Facts"):

| Request | API call | Kept |
|---|---|---|
| `Page(Search(q))` | `GET /search?query={q}&types=TRACKS,ALBUMS,ARTISTS,PLAYLISTS&limit={n}&offset=0` | `tracks`, `albums`, `artists`, `playlists`: each `{limit, offset, totalNumberOfItems, items}`; `topHit` `{type, value}` mapped by `type` (`TRACKS`, `ALBUMS`, `ARTISTS`, `PLAYLISTS`; any other type, `topHit: null` or a missing key → `None`); `videos` ignored |
| `More { SearchTracks(q) … }` | `GET /search/{tracks,albums,artists,playlists}?query={q}&limit=…&offset=…` | `{limit, offset, totalNumberOfItems, items}` with bare items |

Items map with 0006's mappings (tracks: 0004's, albums and artists: 0006's summaries, playlists: `own: false`). Tracks without `STEREO` in `audioModes` are dropped from *Tracks* and as a top hit (the hit is then `None`); the dropped rows still count in `offset`, as 0006's short pages do. The largest page is **1000** on both endpoints (the probe's `limit=1000` was accepted), so `TIDAL_PLAYER_PAGE_SIZE` is clamped to 1000. The per-type endpoints add an `artist` field and leave out the track's `album.releaseDate`; the mapping ignores both. The query is URL-encoded by the HTTP client (spaces, `&`, `/`, non-ASCII). `countryCode` as everywhere.

### Rendering

Below the title row: the input row (`Search: <query>▏` with a cursor block while it has the focus, dim without), the top-hit row (`Top hit: <row as in its window> · <kind>`, highlighted when focused; no row when there is none), then the four windows in a 2 × 2 grid when the inner width is at least 60 columns: *Tracks* | *Albums* over *Artists* | *Playlists*, each half the width and half the height (spotify-player's horizontal layout without its Shows/Episodes row). Below 60 columns only the focused window is drawn under the input and the top hit, with `‹Tab›` as in 0006. Nothing panics at any size.

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Search · "pierce the veil"                                                    │
│Search: pierce the veil                                                       │
│Top hit: Pierce The Veil · artist                                             │
│┌Tracks (300)──────────────────────┐┌Albums (41)──────────────────────────────┐│
││Hell Above        Pierce The Veil ││Collide With The Sky  Pierce The Veil 2012││
││King for a Day    Pierce The Veil ││Misadventures         Pierce The Veil 2016││
│┌Artists (6)───────────────────────┐┌Playlists (300)──────────────────────────┐│
││Pierce The Veil                   ││This Is Pierce The Veil              50  ││
│…                                                                             │
```

## Acceptance criteria

Protocol and types (`tidal_player_core`, pure):

- **AC1** — `PageRequest::Search`, `PageData::Search`, every `TopHit` variant and the four `ListRef::Search*` variants survive a JSON round trip and 0005's codec (0006 AC1's tests, new rows)

API (`tidal-player-api::library`, wiremock fixtures from the probe):

- **AC2** — `Page(Search(q))` sends one `GET /search` with `query` URL-encoded (rows: `pierce the veil`, `AC/DC`, `Sigur Rós`, `a&b`), `types`, `countryCode`, `limit = min(page size, largest page)`, `offset=0`; returns the four lists with their totals and the top hit (one row per `type`, an unknown `type`, `topHit: null` and a missing key → `None`); a Dolby-Atmos-only track dropped from *Tracks* and as a top hit; an empty result (`topHit: null`, totals 0) is four empty lists; a `400` → `Tidal refused the search: <userMessage>`; `LoginRequired` and transient errors unchanged (table)
- **AC3** — `More` on each `ListRef::Search*` sends `GET /search/{type}` with `query`, `limit` clamped, `offset`; unwraps bare items (the per-type track shape with `artist` and no `album.releaseDate` maps); drops Dolby-Atmos-only tracks; an `offset` past the total returns no items (table over the four lists)

Player (`tidal-player`):

- **AC4** — A `Library { Page(Search(…)) }` and a `More` on a search list are answered like any 0006 request: one `LibraryReply` to the sender, beside the input loop (0006 AC8's harness, new rows)

Client model (`tidal_player_core::ui`, pure):

- **AC5** — `g s` pushes an empty search page with the input focused and emits nothing; `g s` on a search page on top focuses its input and pushes nothing; back and forth through the history keeps query, results, cursors and focus with no request
- **AC6** — Input editing (table): printable keys, `Space`, `q`, `n`, `g` append; `Backspace` deletes one character and on an empty input does nothing (no page pop); `C-u` clears; the 201st character is not appended; `C-c` quits; `C-q` goes back; no key but `Enter` emits a request, and none emits a player command
- **AC7** — Sending: `Enter` on an empty or all-space input emits nothing; otherwise one `Library { Page(Search(trimmed)) }` with a fresh ID, windows `Loading…`; the reply fills the top hit and the four windows and focuses the top hit, else the first non-empty window, else the input; a second `Enter` before the first reply makes the first reply be dropped; a reply for an earlier search on a page lower in the history still lands on that page
- **AC8** — Windows: `Tab`/`BackTab` cycle input → Top hit (skipped when absent) → Tracks → Albums → Artists → Playlists → input; `/` on a window focuses the input; `Esc` on the input focuses what a reply would focus when there are results; scrolling a window emits `More { SearchTracks(q) … }` (and the other three) per 0006 AC10's rules
- **AC9** — Rows act as on 0006 pages (table): `Enter` on track *i* sends `LoadQueue { the loaded track rows, start: i }` with no `More` first, and after the reply no `More` is sent for it; `Enter` on a top-hit track among the loaded rows does the same, one not among them sends `LoadQueue { [top hit] + loaded rows, start: 0 }`; `Enter` on a top-hit album, playlist or artist pushes its page; `Enter` on an album, playlist, artist pushes its page; `Z` sends 0006's `Open`; the actions popup lists 0006's actions, a search playlist as not own
- **AC10** — Disconnected and session expired: `Enter` while disconnected fails the page with the disconnected message and sends nothing; the next `Welcome` re-sends the search if the page on top failed that way (0006 AC16's rules)

TUI rendering and runtime (`tidal-player`):

- **AC11** — Key decoding: `C-u` decodes; pasted text (bracketed paste) reaches the input as characters (`crates/app/src/input.rs`, table)
- **AC12** — Rendering (`insta`, 80×24, reviewed by eye, plus `contains` checks): empty search page with focused input; loading; results with a top hit and four windows with totals; results without a top hit; each `No … found`; a failed search; 50×20: input plus the focused window with `‹Tab›`. No panic from 0×0 to 120×40
- **AC13** — Wiring: an attached client's search is answered from the test player's wiremock API and opens no session store (0006 AC19's test, a `g s`, typed query, `Enter`)

Docs:

- **AC14** — `docs/tui.md` documents the search page, its keys (`g s`, input editing, `/`, `Tab`), `Enter` on a track queuing the loaded results, and the messages; `CLAUDE.md` "Status" names this spec; this spec links to them

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| No top hit, or one of a kind not listed (a video) | No *Top hit* row; `Tab` skips it |
| A query that matches nothing | Each window says `No … found`; the input keeps the focus |
| A query with `&`, `/`, `#`, `?`, quotes, emoji or CJK | Sent URL-encoded; results as Tidal returns them (AC2) |
| A pasted Tidal link as the query | Searched as text (opening links stays with `o`/`O`; see "Out of scope") |
| `Enter` pressed repeatedly | Each sends a search; only the latest result is applied (AC7); the player runs a client's requests one at a time (0006 AC8) |
| `429` | The page shows `Tidal answered 429: try again in a moment`; nothing retries |
| A result that is a Dolby-Atmos-only track | Not listed (and not the top hit); the title's total is Tidal's, so it can be a few more than the rows (as 0006's favorite tracks) |
| A 200-character query | Tidal answers no results: `No … found` |
| Thousands of track results, `Enter` on one | Only the loaded rows are queued, and nothing more is fetched for that queue later (decision 4) |
| A result track not streamable | Dimmed, queued, skipped by the player (0004 AC7) |
| Narrow terminal | Input plus one window (AC12); below 6 inner rows only the playback window (0004) |
| Disconnected while searching | The page fails with the disconnected message; re-sent on reconnect if on top (AC10) |

## Test plan

Each test is named after its criterion (`ac7_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0006. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/search/`, written from the probe's recorded shapes.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | stub `Serialize` writes `null` |
| AC2 | `crates/api/tests/search.rs` :: `ac2_search_page` (table) | path, encoded query, params, four lists, errors | stub sends `query` unencoded and returns empty lists |
| AC3 | `crates/api/tests/search.rs` :: `ac3_search_more` (table) | per-type path, clamp, offset, unwrapping, Atmos drop | stub ignores `offset` |
| AC4 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (search rows) | one reply to the sender | stub answers `Search` with an error |
| AC5 | `crates/core/src/ui/search/tests.rs` :: `ac5_search_page_history` (table) | push, refocus, kept state | stub `g s` does nothing |
| AC6 | `crates/core/src/ui/search/tests.rs` :: `ac6_input_editing` (table) | typed text, no commands | stub lets `q` quit |
| AC7 | `crates/core/src/ui/search/tests.rs` :: `ac7_send_and_reply` (table) | one request, trimmed, stale drop, focus | stub applies every reply |
| AC8 | `crates/core/src/ui/search/tests.rs` :: `ac8_windows_and_scroll` (table) | focus cycle, `/`, `Esc`, `More` | stub `Tab` skips the input |
| AC9 | `crates/core/src/ui/search/tests.rs` :: `ac9_rows_act` (table) | exact command or page per row | stub `Enter` fetches the whole track list |
| AC10 | `crates/core/src/ui/search/tests.rs` :: `ac10_disconnected` (table) | nothing sent; one re-send | stub sends while disconnected |
| AC11 | `crates/app/src/input.rs` :: `ac18_key_events` (new rows) | decoded keys, paste | stub drops `C-u` and paste |
| AC12 | `crates/app/src/ui.rs` :: `ac12_search_80x24` (one snapshot per row), `ac12_search_narrow_50x20`, `ac17_no_panic_any_size` (search rows) | `contains` + snapshots | stub render draws no input row |
| AC13 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (search step) | answered; no session opened | stub client drops the search |
| AC14 | — reviewed at acceptance | docs match this spec, linked | — |

Checked by hand at acceptance with a real account (results in the PR description): searching an artist, an album title and a phrase gives the same first results as the Tidal app; scrolling *Tracks* to the end of a large result stops cleanly; `Enter` on a result track plays it with the rest of the loaded results after it; non-ASCII queries (`Sigur Rós`, `米津玄師`) find their artist.

## Crate placement

- `tidal-player-core`: `library` (`PageRequest::Search`, `PageData::Search`, `ListRef::Search*`), `ui` (`PageKind::Search`, the input and its focus, `WindowKind::{SearchTracks, SearchAlbums, SearchArtists, SearchPlaylists}`; a `ui/search` module with its tests). No I/O, no new dependency
- `tidal-player-api::library` (or a `search` module beside it): the two calls and their DTO mapping
- `tidal-player`: `player_runtime` (the `Library` trait answers the new requests), `ui.rs`/`ui/pages.rs` (search page), `input.rs` (`C-u`, paste)
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-08, from code and docs):

- spotify-player (`README.md`, `spotify_player/src/ui/page.rs` `render_search_page`, read 2026-10-08): `SearchPage` is `g s`; `/` is `Search`, "a popup for searching in the current page"; the page opens with the focus on the input, `Enter` searches, `FocusNextWindow`/`FocusPreviousWindow` move from the input to the result windows; windows in the order Tracks, Albums, Artists, Playlists, Shows, Episodes, in two columns in its horizontal layout; while loading or failed it shows `Loading...` / `Search failed` and no windows. `C-s` is its shuffle, as ours (0004)
- tidalt's search (above) called `GET /v1/search` with `query`, `limit=20`, `countryCode`, `types=TRACKS,ARTISTS,ALBUMS,PLAYLISTS` and decoded `tracks.items`, `artists.items`, `albums.items`, `playlists.items`; it worked against the live API for tidalt's users, with the same client ID as ours
- 0006's machinery covers the rest: `Library` requests run as jobs in the player, `ListPage` paging, request IDs, the actions popup

Verified on 2026-10-08 against the **live API** by `scripts/tidal-search-probe.sh`, run by the user (country NG; the script was deleted after this update). Fixtures under `crates/api/tests/fixtures/search/` are written from these shapes (catalogue data is public; user IDs replaced):

- `GET /v1/search?query=…&types=…&limit=…&offset=…&countryCode=…` → `200 {artists, albums, playlists, tracks, videos, topHit}`; each list `{limit, offset, totalNumberOfItems, items}` with bare items: artists as 0006's (`id`, `name`, …), albums as 0006's (`type` `ALBUM`/`EP`/`SINGLE`, `releaseDate`, `numberOfTracks`, `duration`, `artists[]`), tracks as 0004's (`artists[].id`, `album {id, title, releaseDate}`, `audioModes`, `version`), playlists (`uuid`, `title`, `numberOfTracks`, `duration`, `type` `EDITORIAL`/`ARTIST`/`PODCAST`, `creator: {}`). Default page 10
- **`types` limits the lists**: `types=TRACKS` answers the other lists empty with total 0; without `types` the `videos` list is filled (14 music videos for "pierce the veil")
- **`topHit`**: `{value, type}` with `value` the full item of that kind; `type` `ARTISTS` ("pierce the veil", "love", "Sigur Rós", "米津玄師"), `ALBUMS` ("collide with the sky", "a", "AC/DC"), `TRACKS` ("hell above", "this is pierce the veil", and with `types=TRACKS`); **`null`** when nothing matches (empty, no-match and 200-character queries). `PLAYLISTS` was not seen (no query made a playlist the top hit); it is mapped by the same rule
- **Page size and depth**: `limit` 50, 100, 300 and 1000 accepted on `/v1/search` and on `/v1/search/tracks`; totals stayed at or under 300 for every query ("love": 295 tracks, 189 albums, 125 artists, 114 playlists; "a" and "collide with the sky": 300 tracks); `/v1/search/tracks?offset=250&limit=50` → 45 items, `offset=300`, `1000`, `10000` → `200` with no items and the same total
- **Per-type endpoints** `GET /v1/search/{tracks,albums,artists,playlists}` exist and answer `{limit, offset, totalNumberOfItems, items}` with the same order and totals as the combined endpoint; their tracks and albums add `artist` (the main artist), tracks leave out `album.releaseDate`, playlists carry `creator: {id: 0}` for editorial ones
- **Track results include Dolby-Atmos-only copies** ("hell above": a `LOW`, `["DOLBY_ATMOS"]` copy at position 2) and AI-flagged tracks (`ai: true`, kept)
- **Queries**: empty → `200`, all empty, `topHit: null` (the client never sends one); `AC/DC`, `Sigur Rós` and `米津玄師` URL-encoded find AC/DC, Sigur Rós and Kenshi Yonezu first; a 200-character query → `200` with no results. No `400` was seen; the `400` message rule stays for safety

Not verified:

- A `PLAYLISTS` top hit (mapped like the others; tested on a fixture only)
- Whether a user-created playlist in results carries the owner's `creator.id` (so decision 5 stays: never own)
- Whether the ~300 cap is fixed or depends on the query (the client follows `totalNumberOfItems` either way)

## Decisions (answered by the user, 2026-10-08; folded into the body above)

1. **Key to open search**: *`g s`*, as proposed; `/` stays for 0008's in-page filter
2. **When to search**: *on `Enter`*, as proposed
3. **Layout**: *2 × 2 grid*, Tracks | Albums over Artists | Playlists
4. **`Enter` on a result track**: *queue the current results only* ("just add current search results, do not fetch more after enqueuing"): the loaded rows of *Tracks*, nothing fetched before or after
5. **Own playlists in results**: not answered; the body keeps the proposal (never marked own): the probe showed only editorial playlists, with `creator: {}` or `{id: 0}`
6. **Tidal's top hit**: *shown*: a one-row *Top hit* between the input and the windows, first in the `Tab` cycle and focused after a search, acting as a row of its kind

## Out of scope

- An in-page filter (`/` on other pages, spotify-player's `Search` popup) and configurable keys (0008)
- Search history and remembering the last query across runs (0009)
- Opening a pasted Tidal link from the search input (stays with `o`/`O`)
- Videos, mixes and radio among results (0011)
- One-shot search from the command line (`tidal-player playback search …`)
- Search suggestions / autocomplete as you type
