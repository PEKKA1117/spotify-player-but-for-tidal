# 0007 — Search

- **Status**: draft (2026-10-08). Open decisions are under "Decisions (open)"; the API shapes under "Not verified" wait on a live-API probe
- **Owner**: tech-lead (primary session)
- **Depends on**: 0006 (implemented: pages, the history, windows that load as you scroll, `Enter`/`Z`/actions on rows, `Library`/`LibraryReply`)
- **User docs**: [`docs/tui.md`](../tui.md) gains a "Search" section and the new key (AC14)

## Context

0006 lets the user browse what is already theirs. To play anything else they still paste a link or an ID (0004 `o`/`O`). This spec adds spotify-player's **search page**: type a query, get Tidal's matching tracks, albums, artists and playlists in four windows, and use them exactly as rows of any other page (play, queue, open, actions popup).

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
8. **Videos among track results** → the tracks window keeps tracks only (AC3)

## Behaviour

### The search page

`g s` opens the **search page** (spotify-player's `SearchPage`), pushed on the history like any 0006 page; if the page on top is already a search page, `g s` gives its input the focus instead. The page has a one-row **input** above four windows:

| Window | Rows (0006's) | Title |
|---|---|---|
| **Tracks** | track rows | `Tracks (1 234)` |
| **Albums** | album rows | `Albums (87)` |
| **Artists** | artist rows | `Artists (12)` |
| **Playlists** | playlist rows | `Playlists (300)` |

- The page opens with an empty input that has the **focus**, and the windows show nothing. The title row reads `Search`; after a search, `Search · "<query>"`
- **Input keys** (while the input has the focus): printable characters (`Space` included) and pasted text are appended; `Backspace` deletes the last character; `C-u` clears the input; `Enter` sends the search; `Tab`/`BackTab` move the focus to the windows; `Esc` gives the focus to *Tracks* if there are results, else does nothing. Every other key is ignored, so `q`, `n`, `Space`, `g …` and `Backspace` on an empty input do not act (`C-c` still quits, `C-q` still goes back)
- **Sending**: `Enter` with an input that is empty after trimming spaces does nothing. Otherwise the trimmed query (at most 200 characters, longer input is not accepted: typing stops at 200) is sent as `Library { Page(Search(query)) }`; all four windows show `Loading…`, then the first page of each list, and the focus moves to *Tracks* (or to the first non-empty window). A new search on the same page replaces the results; cursors return to the top
- **Windows**: `Tab`/`BackTab` cycle the focus over input → *Tracks* → *Albums* → *Artists* → *Playlists* → input, wrapping (spotify-player's order). With the focus on a window, every 0006 key works as on any page: cursor keys, `Enter`, `Z`, `g a`/`C-Space`, `a`, `Backspace` (back), `g l`, `z`, … `/` gives the input the focus again (spotify-player uses `/` for its in-page search popup, 0008; on this page it means "search again")
- **History**: going back to a search page shows its query, results, cursors and focus as they were, without searching again. `g s` from another page pushes a new, empty search page (the earlier one stays in the history below)
- **Empty results**: a window with no results says `No tracks found`, `No albums found`, `No artists found`, `No playlists found`; a failed search shows its message in the page (`Could not search: <reason>`), as a failed 0006 page does

### Lists load as you scroll

Each window is a 0006 list: the first page comes with the search (page size `TIDAL_PLAYER_PAGE_SIZE`, clamped per endpoint), the next when the cursor nears the last loaded row, `Loading more…`, failures as the last row, rows already loaded skipped by ID. A list stops at the smaller of Tidal's total and the deepest offset Tidal answers (see "Not verified": Tidal may report totals it does not page to).

### Playing and queueing from results

0006's table holds, with one difference for `Enter` on a track (decision 4):

| Key | On | Does |
|---|---|---|
| `Enter` | a track | Replaces the queue with the **loaded** rows of *Tracks*, in result order, and plays the chosen one: `LoadQueue { tracks, start: index }`. Nothing more is fetched first (a search for "love" can match thousands of tracks) |
| `Enter` | an album, playlist or artist | Opens its page (0006) |
| `Z`, `C-z` | a track, album or playlist | Adds it at the end of the queue (0006) |
| `g a`, `C-Space` | any row | 0006's actions popup for that kind of row (a search playlist is never *own*: no *Delete playlist*, and *Add to favorites*/*Remove from favorites* are both shown) |

### Talking to the player

0006's `Library` request, two additions:

- `PageRequest::Search(String)` → `PageData::Search { tracks: ListPage<Track>, albums: ListPage<AlbumSummary>, artists: ListPage<ArtistRef>, playlists: ListPage<PlaylistSummary> }`
- `ListRef::SearchTracks(String)`, `SearchAlbums(String)`, `SearchArtists(String)`, `SearchPlaylists(String)` for `More`

Errors are 0006's (`Could not reach Tidal: …`, `Tidal answered 429: try again in a moment`, the session-expired message); a request Tidal refuses as invalid (`400`) shows `Tidal refused the search: <userMessage>`.

### What the player fetches

To be confirmed by the probe (see "Not verified"); proposed from tidalt's working code:

| Request | API call | Kept |
|---|---|---|
| `Page(Search(q))` | `GET /search?query={q}&types=TRACKS,ALBUMS,ARTISTS,PLAYLISTS&limit={n}&offset=0` | `tracks`, `albums`, `artists`, `playlists`: each `{limit, offset, totalNumberOfItems, items}`; `topHit` and `videos` ignored |
| `More { SearchTracks(q) … }` | `GET /search/{tracks,albums,artists,playlists}?query={q}&limit=…&offset=…` | `{limit, offset, totalNumberOfItems, items}` with bare items |

Items map with 0006's mappings (tracks: 0004's, albums and artists: 0006's summaries, playlists: `own: false`). The query is URL-encoded by the HTTP client (spaces, `&`, `/`, non-ASCII). `countryCode` as everywhere.

### Rendering

Below the title row: the input row (`Search: <query>▏` with a cursor block while it has the focus, dim without), then the four windows in a 2 × 2 grid when the inner width is at least 60 columns: *Tracks* | *Albums* over *Artists* | *Playlists*, each half the width and half the height (spotify-player's horizontal layout without its Shows/Episodes row). Below 60 columns only the focused window is drawn under the input, with `‹Tab›` as in 0006. Nothing panics at any size.

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Search · "pierce the veil"                                                    │
│Search: pierce the veil                                                       │
│┌Tracks (300)──────────────────────┐┌Albums (41)──────────────────────────────┐│
││Hell Above        Pierce The Veil ││Collide With The Sky  Pierce The Veil 2012││
││King for a Day    Pierce The Veil ││Misadventures         Pierce The Veil 2016││
│┌Artists (6)───────────────────────┐┌Playlists (300)──────────────────────────┐│
││Pierce The Veil                   ││This Is Pierce The Veil              50  ││
│…                                                                             │
```

## Acceptance criteria

Protocol and types (`tidal_player_core`, pure):

- **AC1** — `PageRequest::Search`, `PageData::Search` and the four `ListRef::Search*` variants survive a JSON round trip and 0005's codec (0006 AC1's tests, new rows)

API (`tidal-player-api::library`, wiremock fixtures from the probe):

- **AC2** — `Page(Search(q))` sends one `GET /search` with `query` URL-encoded (rows: `pierce the veil`, `AC/DC`, `Sigur Rós`, `a&b`), `types`, `countryCode`, `limit = min(page size, largest page)`, `offset=0`; returns the four lists with their totals; a `400` → `Tidal refused the search: <userMessage>`; `LoginRequired` and transient errors unchanged (table)
- **AC3** — `More` on each `ListRef::Search*` sends `GET /search/{type}` with `query`, `limit` clamped, `offset`; unwraps bare items; drops non-track items from tracks; a page beyond the deepest offset Tidal answers returns no items and `total` = rows so far (table over the four lists)

Player (`tidal-player`):

- **AC4** — A `Library { Page(Search(…)) }` and a `More` on a search list are answered like any 0006 request: one `LibraryReply` to the sender, beside the input loop (0006 AC8's harness, new rows)

Client model (`tidal_player_core::ui`, pure):

- **AC5** — `g s` pushes an empty search page with the input focused and emits nothing; `g s` on a search page on top focuses its input and pushes nothing; back and forth through the history keeps query, results, cursors and focus with no request
- **AC6** — Input editing (table): printable keys, `Space`, `q`, `n`, `g` append; `Backspace` deletes one character and on an empty input does nothing (no page pop); `C-u` clears; the 201st character is not appended; `C-c` quits; `C-q` goes back; no key but `Enter` emits a request, and none emits a player command
- **AC7** — Sending: `Enter` on an empty or all-space input emits nothing; otherwise one `Library { Page(Search(trimmed)) }` with a fresh ID, windows `Loading…`; the reply fills the four windows and focuses *Tracks* (the first non-empty window; the input if all are empty); a second `Enter` before the first reply makes the first reply be dropped; a reply for an earlier search on a page lower in the history still lands on that page
- **AC8** — Windows: `Tab`/`BackTab` cycle input → Tracks → Albums → Artists → Playlists → input; `/` on a window focuses the input; `Esc` on the input focuses *Tracks* when there are results; scrolling a window emits `More { SearchTracks(q) … }` (and the other three) per 0006 AC10's rules
- **AC9** — Rows act as on 0006 pages (table): `Enter` on track *i* sends `LoadQueue { the loaded track rows, start: i }` with no `More` first; `Enter` on an album, playlist, artist pushes its page; `Z` sends 0006's `Open`; the actions popup lists 0006's actions, a search playlist as not own
- **AC10** — Disconnected and session expired: `Enter` while disconnected fails the page with the disconnected message and sends nothing; the next `Welcome` re-sends the search if the page on top failed that way (0006 AC16's rules)

TUI rendering and runtime (`tidal-player`):

- **AC11** — Key decoding: `C-u` decodes; pasted text (bracketed paste) reaches the input as characters (`crates/app/src/input.rs`, table)
- **AC12** — Rendering (`insta`, 80×24, reviewed by eye, plus `contains` checks): empty search page with focused input; loading; results in four windows with totals; each `No … found`; a failed search; 50×20: input plus the focused window with `‹Tab›`. No panic from 0×0 to 120×40
- **AC13** — Wiring: an attached client's search is answered from the test player's wiremock API and opens no session store (0006 AC19's test, a `g s`, typed query, `Enter`)

Docs:

- **AC14** — `docs/tui.md` documents the search page, its keys (`g s`, input editing, `/`, `Tab`), `Enter` on a track queuing the loaded results, and the messages; `CLAUDE.md` "Status" names this spec; this spec links to them

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| A query that matches nothing | Each window says `No … found`; the input keeps the focus |
| A query with `&`, `/`, `#`, `?`, quotes, emoji or CJK | Sent URL-encoded; results as Tidal returns them (AC2) |
| A pasted Tidal link as the query | Searched as text (opening links stays with `o`/`O`; see "Out of scope") |
| `Enter` pressed repeatedly | Each sends a search; only the latest result is applied (AC7); the player runs a client's requests one at a time (0006 AC8) |
| `429` | The page shows `Tidal answered 429: try again in a moment`; nothing retries |
| Tidal reports a total it does not page to | The list stops where Tidal stops answering (AC3) |
| Thousands of track results, `Enter` on one | Only the loaded rows are queued (decision 4) |
| A result track not streamable | Dimmed, queued, skipped by the player (0004 AC7) |
| Narrow terminal | Input plus one window (AC12); below 6 inner rows only the playback window (0004) |
| Disconnected while searching | The page fails with the disconnected message; re-sent on reconnect if on top (AC10) |

## Test plan

Each test is named after its criterion (`ac7_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0006. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/search/`, written from the probe's recorded shapes.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | stub `Serialize` writes `null` |
| AC2 | `crates/api/tests/search.rs` :: `ac2_search_page` (table) | path, encoded query, params, four lists, errors | stub sends `query` unencoded and returns empty lists |
| AC3 | `crates/api/tests/search.rs` :: `ac3_search_more` (table) | per-type path, clamp, offset, unwrapping, depth limit | stub ignores `offset` |
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

Not verified (to be answered by a live-API search probe, run by the user, before approval):

- The exact `/v1/search` envelope (the `topHit`, `videos` keys; whether each list carries `totalNumberOfItems`); whether `types` limits the response to those lists
- The largest `limit` per list on `/v1/search` and on `/v1/search/{type}`, and whether the per-type endpoints exist under that path
- How deep `offset` goes: whether Tidal caps search results (e.g. at 300) whatever `totalNumberOfItems` says
- Whether track results ever contain videos or non-track items
- What an empty query, a one-letter query and a 200-character query answer
- Whether playlist results carry `creator.id` (to mark the user's own playlists, decision 5)

## Decisions (open)

Each has a recommendation; the body above follows it until the user answers.

1. **Key to open search**: `g s` (spotify-player's `SearchPage`; recommended) — or `/` everywhere. Recommended `g s`, keeping `/` for 0008's in-page filter (spotify-player's `Search`)
2. **When to search**: on `Enter` (spotify-player; recommended: one request per search, no rate-limit risk) — or as you type, after a 300 ms pause
3. **Layout**: 2 × 2 grid Tracks | Albums over Artists | Playlists (recommended, spotify-player's horizontal layout) — or four windows side by side as the library's three
4. **`Enter` on a result track**: queue the **loaded** track results after it (recommended: a search is not a list the user chose; thousands of matches would otherwise load first) — or 0006's rule (fetch every result first, then queue) — or play the track alone and let autoplay continue
5. **Own playlists in results**: never marked own (recommended for now) — or compare `creator.id` with the user ID if the probe shows it, enabling *Delete playlist* on them
6. **Tidal's top hit** (`topHit`): not shown (recommended; one more window for one row) — or as the first row of its window, highlighted

## Out of scope

- An in-page filter (`/` on other pages, spotify-player's `Search` popup) and configurable keys (0008)
- Search history and remembering the last query across runs (0009)
- Opening a pasted Tidal link from the search input (stays with `o`/`O`)
- Videos, mixes and radio among results (0011); Tidal's top hit (decision 6)
- One-shot search from the command line (`tidal-player playback search …`)
- Search suggestions / autocomplete as you type
