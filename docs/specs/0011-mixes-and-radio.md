# 0011 — Mixes and radio

- **Status**: implemented (2026-10-09; the manual checks under "Test plan" are run on the user's machine); API shapes verified by the live probe (2026-10-09, "Facts")
- **Owner**: tech-lead (primary session)
- **Depends on**: 0004 (implemented: the queue, autoplay's track radio), 0006 (implemented: pages, the history, lists that load as you scroll, the actions popup, `Library`/`LibraryReply`), 0008 (implemented: the keymap, `[[actions]]`, the keys help)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Mixes" and "Radio" sections and the new keys; [`docs/config.md`](../config.md) gains `MixesPage` and `GoToRadio` (AC13)

## Context

Tidal builds mixes for each user (*My Daily Discovery*, *My New Arrivals*, daily mixes by genre or era) and a radio for any track or artist. So far the player uses them only behind autoplay (0004: the track radio at the end of the queue). This spec makes them browsable, as spotify-player does with its radio page:

- a **mixes page** listing the user's mixes, and a **mix page** for each, a track list like an album's
- **radio pages** for a track or an artist, opened with spotify-player's *Go to radio* action (`r`)

Mix links stay refused as items (decision 5).

It adds no new mechanism where 0006 has one: mix and radio pages are pages in the history, fetched by the player, and their rows are 0006's rows with 0006's keys and actions. Unlike 0006's lists they arrive whole: Tidal serves a mix in one response and caps a radio at 100 tracks ("Facts").

### What tidalt did

Read from tidalt's `internal/tidal/api.go` (`GetMixes`, `GetMixTracks`, `GetTrackRadio`), `internal/ui/{keys,overlay}.go`, `docs/architecture.md` "Daily Mixes" and the commits named:

- **Mixes from a removed API** (`b1eddc8`, `40bf41e`, then `e253b75`): the v2 `openapi.tidal.com/v2/userRecommendations/me/relationships/myMixes` returned bare track IDs, fetched one `/v1/tracks/{id}` per track, then re-sorted. Tidal removed the resource and the view came up empty. `e253b75` moved to v1: `GET /v1/pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US` for the list (every module's `pagedList.items`, duplicates and ID-less entries skipped, `mixType` containing `VIDEO` dropped: 9 mixes kept, 8 video mixes dropped on the author's account), `GET /v1/mixes/{id}/items` for the tracks (full tracks, `{item, type}`, non-`track` dropped)
- **`Enter` on a mix replaced the queue** with its tracks at once: no way to see what a mix holds before playing it, and no choosing a track in it
- **The mix list was fetched once** (at start, and when the section was first opened) and never refreshed, though daily mixes change every day
- **Radio played at once** (`r`, *Start radio from this*): `GET /v1/tracks/{id}/radio?limit=100` replaced the queue, marked `radio · unsaved`. Track radio only: no artist radio
- **Mix links** (`tidal.com/mix/<id>`) were accepted in its search input, loading the mix into the queue

### What could go wrong here, and the criterion that covers it

1. **A removed or changed endpoint** (tidalt: v2 `userRecommendations` → `404`, an empty view with no message) → v1 endpoints from the probe, fixtures from its shapes, and a failed fetch says so in the page (AC2, AC10)
2. **Video mixes and video items queued as tracks** → video mixes are left out of the list; items that are not tracks, or tracks without `STEREO`, are dropped from mix and radio lists (AC2, AC3, AC4)
3. **One request per track** (tidalt's v2 fan-out) → a mix's tracks come as full tracks from its items in one request (AC3)
4. **A mix played before it can be seen** (tidalt `Enter`) → `Enter` on a mix opens its page; only `Enter` on a track plays, with the rest of the mix queued (0006's rule) (AC7)
5. **Stale daily mixes** (tidalt: fetched once) → opening the mixes page always fetches it, as every 0006 page (AC6)
6. **Radio that throws the queue away** (tidalt `r`) → `r` (*Go to radio*) opens a page; the queue changes only on `Enter` or `Z`, as everywhere (AC6)
7. **A seed with no radio** (`404`, "Track radio cannot be generated", 0004 "Facts") → an empty radio page saying so, not a failure (AC4)
8. **Paging a mix repeats it**: `/mixes/{id}/items` ignores `limit` and `offset` and always answers the whole mix (the probe: `offset=50` and `offset=100` returned the same 10 tracks), so 0006's scroll loading would show it twice or loop → one request per mix, never a second (AC2)
9. **A list that looks unfinished**: a radio reports 98 or 100 tracks, and dropped rows would leave the client asking for more → mix and radio lists carry the number of rows kept as their total, so the client never asks for more (AC3, AC4)

## Behaviour

### Pages

Three new page kinds, opened and kept in the history as 0006's:

| Page | Opened with | Windows | Title row |
|---|---|---|---|
| Mixes | `g m` (`MixesPage`*), from anywhere | the user's mixes | `Mixes · 7 mixes` |
| Mix | `Enter` on a mix; *Open* in its popup | the mix's tracks | `<title> · 10 tracks` |
| Radio | `r` (*Go to radio*) on a selected track or artist, or the popup entry | the radio's tracks | `<track title> Radio · <artists>` / `<artist name> Radio` (spotify-player's `<name> Radio`) |

- **Mix rows**: title, then the subtitle (Tidal's `subTitle`: `<artist>, <artist>, <artist> and more` for a daily mix, a sentence for *My Daily Discovery*), truncated with `…` as 0006's rows. Tidal's order
- The mix page's title row is the mix's title; the subtitle shows under it as a second, dim title row when the page is at least 8 rows tall
- **Track rows** on mix and radio pages: 0006's track rows (non-streamable dimmed)
- Empty: `No mixes yet`, `This mix has no tracks`, `No radio for this track`, `No radio for this artist`. Failures in the page: `Could not load the mixes: <reason>`, `Mix <id> was not found`, `Track 1 was not found`, `Artist 1 was not found`
- Every page fetches fresh when opened (0006, 0009 decision 3); going back shows it as it was

### Whole lists, no scroll loading

The three pages arrive whole: the player answers each with every row, and the list's `total` is the number of rows it holds, so 0006's scroll loading and whole-list fetch never ask for more (no `More`, no `ListRef` for them). The mixes list is a few dozen at most (15 entries on the probe account, 7 of them audio); a mix is one response (10 to 100 tracks seen); a radio is at most 100 tracks. `page_size` does not apply.

### Playing, queueing and actions

0006's tables hold, with mixes as a new row kind:

| Key | On | Does |
|---|---|---|
| `Enter` | a mix | Opens its page |
| `Enter` | a track on a mix or radio page | `LoadQueue { every track of the list, start: index }` (the list is whole: nothing is fetched first) |
| `Z`, `C-z` | a mix | Nothing, as on an artist: a mix is not an item (decision 5); open it and use `Enter` or `Z` on its tracks |
| `r` | a track or an artist row (any page, the queue included) | Opens its radio page (*Go to radio*) |

| Item | Actions, in this order |
|---|---|
| Mix | *Open* |
| Track (browse page, queue entry, playing track) | 0006's list with ***Go to radio*** after the *Go to artist* entries |
| Artist | *Open*, ***Go to radio***, *Add to favorites*/*Remove from favorites* |

- *Go to radio* on a track opens `Radio(track)`; on an artist `Radio(artist)`; on an album, playlist or mix there is none (decision 4). It is spotify-player's `GoToRadio` action (it was in 0008's skipped list), bound by default to **`r`** on the selected row (decision 3; tidalt's key). Other keys bind it with `[[actions]] action = "GoToRadio"`, for the playing track with `target = "PlayingTrack"`; `[[keymaps]] key_sequence = "r"`, `command = "None"` unbinds it
- Favoriting mixes is out of scope: a mix row's popup has no favorite entries

### Talking to the player

0006's `Library` request, additions:

- `PageRequest::Mixes` → `PageData::Mixes { mixes: ListPage<MixSummary> }`
- `PageRequest::Mix(String)` (the mix ID, 30 hex digits as Tidal gives it) → `PageData::Mix { mix: MixSummary, tracks: ListPage<Track> }`
- `PageRequest::TrackRadio(u64)` → `PageData::Radio { seed: RadioSeed::Track(Track), tracks: ListPage<Track> }`; `PageRequest::ArtistRadio(u64)` → `PageData::Radio { seed: RadioSeed::Artist(ArtistRef), tracks }`
- No new `ListRef`: the lists are whole (above)
- `MixSummary { id: String, title, subtitle: Option<String> }`

Errors are 0006's wording; a `404`/`2001` on a mix is `Mix <id> was not found`.

### What the player fetches

`countryCode` on every call, as everywhere. Shapes are the probe's ("Facts").

| Request | API call | Kept |
|---|---|---|
| `Page(Mixes)` | `GET /pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US`; if a module's `pagedList` holds fewer items than its `totalNumberOfItems`, the rest from its `dataApiPath` with `limit`/`offset` (the player's own offset: the answer reports `offset: 0`), until the total | every `MIX_LIST` module's `pagedList.items[]`: `id`, `title`, `subTitle` (empty → `None`); `mixType` containing `VIDEO` dropped; a repeated or empty `id` dropped |
| `Page(Mix(id))` | `GET /pages/mix?mixId={id}&deviceType=BROWSER&locale=en_US` (the `MIX_HEADER` module's `mix`: `title`, `subTitle`) and `GET /mixes/{id}/items` (no `limit`, no `offset`: both are ignored), in parallel | `MixSummary`; `items[]` with `type == "track"`, their `item` by 0004's track mapping, without `STEREO` dropped |
| `Page(TrackRadio(id))` | `GET /tracks/{id}` (0004) and `GET /tracks/{id}/radio?limit=100` (0004 AC27), in parallel | the header track; bare tracks without `STEREO` dropped. The seed is the first radio track and **is kept**: `Enter` on it plays the seed with its radio after it |
| `Page(ArtistRadio(id))` | `GET /artists/{id}` (0006) and `GET /artists/{id}/radio?limit=100`, in parallel | the header artist; bare tracks without `STEREO` dropped |

- `404`/`2001` on `/pages/mix` or `/mixes/{id}/items` → `Mix <id> was not found`; on the radio headers, 0006's `Track 1 was not found` / `Artist 1 was not found`. A **radio** `404`/`2001` (`Track radio cannot be generated…`, `Artist radio cannot be generated for artist [1]`) is an empty list (`No radio for this track` / `… artist`), as autoplay already treats it (0004 AC27)
- Any failing call fails the page (0006); the `ListPage`'s `total` is the number of rows kept
- `/mixes/{id}` does not exist (`404`); the header comes from `/pages/mix` only. Autoplay (0004) is unchanged: it keeps calling the track radio itself

### Keys

| Command | Default keys | Acts in | Help text |
|---|---|---|---|
| `MixesPage`* | `g m` | everywhere | your mixes |

| Action (`[[actions]]`) | Default keys | Target | Help text |
|---|---|---|---|
| `GoToRadio` | `r` | `SelectedItem` | the radio of the selected track or artist |

`g m` and `r` are no binding's prefix and start none (0008's conflict check stays clean). `r` is the first default `[[actions]]` binding: 0008's defaults gain an actions part, shown in the keys help and `examples/keymap.toml` (whose "none by default" comment changes). `GoToRadio` acts in lists ("Acts in" as `ShowActionsOnSelectedItem`); on a row with no radio it does nothing.

### Rendering

As 0006: one window per page, the title row above it, `Loading…`, failures and empty messages in the window. Nothing panics at any size.

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Mixes · 7 mixes                                                               │
│┌Mixes (7)────────────────────────────────────────────────────────────────────┐│
││My Daily Discovery     Songs by new and familiar artists inspired by your li…││
││My Mix 1               Pierce The Veil, Sleeping With Sirens and more         ││
│…                                                                             │
```

## Acceptance criteria

Protocol and types (`tidal_player_core`, pure):

- **AC1** — `MixSummary`, `RadioSeed` and the new `PageRequest` and `PageData` variants survive a JSON round trip and 0005's codec (0001 AC11's and 0005 AC2's tests, new rows); `parse_item` still refuses mix links (0004 AC16's existing row)

API (`tidal-player-api::library`, wiremock fixtures from the probe):

- **AC2** — `Page(Mixes)`: one `GET /pages/my_collection_my_mixes` with `deviceType`, `locale`, `countryCode` when the module holds its total (the probe's 15 of 15); otherwise `dataApiPath` requests with increasing `offset` until the total, though each answer says `offset: 0`; mixes in Tidal's order; `VIDEO_DAILY_MIX` (any `mixType` containing `VIDEO`), repeated and empty IDs dropped; `total` = mixes kept; `LoginRequired` and transient errors as 0006 (table)
- **AC3** — `Page(Mix(id))`: the header from `/pages/mix` and the tracks from **exactly one** `/mixes/{id}/items` with neither `limit` nor `offset` (wiremock `expect(1)`), in parallel; `video` items and tracks without `STEREO` dropped; `total` = tracks kept; `404`/`2001` from either → `Mix <id> was not found`; any failing call fails the page (table)
- **AC4** — Radio: `TrackRadio(id)` sends `GET /tracks/{id}` and `GET /tracks/{id}/radio?limit=100`, `ArtistRadio(id)` `GET /artists/{id}` and `GET /artists/{id}/radio?limit=100`; the seed track kept first; tracks without `STEREO` dropped; `total` = tracks kept (98 for the probe's 98); a radio `404`/`2001` → an empty list; a header `404` → `NotFound` (table)

Player (`tidal-player`):

- **AC5** — The new `Library` requests are answered like any 0006 request: one `LibraryReply` to the sender, beside the input loop (0006 AC8's harness, new rows)

Client model (`tidal_player_core::ui`, pure):

- **AC6** — `g m` pushes the mixes page and emits `Library { Page(Mixes) }` (on top already: nothing); `Enter` on a mix pushes its page and emits `Page(Mix(id))`; going back shows kept rows with no request; scrolling to the end of any of the three pages emits no `More`, and `Enter` on a track sends `LoadQueue` at once (table)
- **AC7** — Rows (table): `Enter` on track *i* of a mix or radio page sends `LoadQueue { the whole list, start: i }` with no `More` before it; `Z`/`C-z` on a mix emits nothing; a mix's popup lists *Open* only
- **AC8** — *Go to radio* (table): listed after the *Go to artist* entries for a browse track, a queue entry and the playing track (`a`), and after *Open* for an artist, absent for albums, playlists and mixes; running it, or `r` on a track row (browse page or queue) or an artist row, pushes `Radio` and emits `Page(TrackRadio(id))` / `Page(ArtistRadio(id))`; `r` on an album, playlist or mix row, or on an empty list, does nothing; nothing changes the queue or sends a command
- **AC9** — Keymap (0008 AC2's table, new rows): the defaults hold `g m` → `MixesPage` and `r` → `GoToRadio` (`SelectedItem`), both in the keys help (`your mixes`, `the radio of the selected track or artist`); a user `[[actions]] action = "GoToRadio"` parses with both targets; `key_sequence = "r"`, `command = "None"` removes the default; a user binding `r x` conflicts with the default `r` (0008's prefix rule) until `r` is unbound; `GoToRadio` is no longer reported as an unsupported spotify-player action; the default keymap has no prefix conflict
- **AC10** — Disconnected and session expired: 0006 AC16's rows for the three new pages (failed with the disconnected message, re-fetched on the next `Welcome` when on top)

TUI rendering and runtime (`tidal-player`):

- **AC11** — Rendering (`insta`, 80×24, reviewed by eye, plus `contains` checks): mixes page loaded (title with count, subtitles); mix page; track radio page; artist radio page; `No mixes yet`; `No radio for this track`; a failed mixes page; the actions popup on a track showing *Go to radio*. 50×20: the mix page. No panic from 0×0 to 120×40 on the three pages
- **AC12** — Wiring: an attached client's `g m` is answered from the test player's wiremock API and opens no session store (0006 AC19's test, a `g m` step)

Docs:

- **AC13** — `docs/tui.md` documents the mixes, mix and radio pages, `g m`, `r`, *Go to radio* and the messages; `docs/config.md` lists `MixesPage` and the `GoToRadio` default and drops `GoToRadio` from the unsupported list; `examples/keymap.toml` shows `g m` and the `r` action; the zh-TW copies and `README.md`/`README.zh-TW.md` match; `CLAUDE.md` "Features" has the entry; this spec links to them

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| A new account with no mixes | `No mixes yet` |
| Only video mixes | Left out: `No mixes yet` |
| A mix of 10 tracks (*My Daily Discovery*) | 10 rows, title `… · 10 tracks`; nothing more is asked |
| A mix link pasted into `o`/`O` or given to `play` | Refused as before: `Not a Tidal track, album or playlist: <item>` (decision 5) |
| A mix that has vanished since the list was fetched (daily mixes rotate) | `Mix <id> was not found` in the page |
| A track Tidal has no radio for | `No radio for this track` in the page; the queue is untouched |
| *Go to radio* on a track that does not stream | The page opens (the radio may still exist) |
| A radio track that is the seed | Kept first, so `Enter` on it plays the seed with its radio after it |
| Daily mixes changed since the page was opened | Going back shows the old rows (0006's history rule); `g m` from another page fetches fresh |
| `429` | The page says `Tidal answered 429: try again in a moment`; nothing retries |
| Session expired, disconnected | 0006's rules (AC10) |
| Narrow terminal | One window, as 0006 (AC11) |

## Test plan

Each test is named after its criterion (`ac4_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0010. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/mixes/`, written from the shapes under "Facts" (catalogue data is public; user IDs replaced; image URLs and tokens dropped).

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) + `crates/core/src/item.rs` :: `ac16_parse_item` (existing mix-link row, unchanged) | round trip; decoding; still refused | stub `Serialize` writes `null` |
| AC2 | `crates/api/tests/mixes.rs` :: `ac2_mixes_list` (table) | path, params, filtering, `dataApiPath` paging, errors | stub keeps video mixes |
| AC3 | `crates/api/tests/mixes.rs` :: `ac3_mix_page` (table) | header, one items request, drops, total, `NotFound` | stub pages with `offset`, so `expect(1)` fails |
| AC4 | `crates/api/tests/mixes.rs` :: `ac4_radio` (table) | paths, seed kept, `404` → empty | stub fails the page on a radio `404` |
| AC5 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (new rows) | one reply to the sender | stub answers the new requests with an error |
| AC6 | `crates/core/src/ui/browse/tests.rs` :: `ac6_mixes_pages` (table) | push, request, kept state, no `More` | stub `g m` does nothing |
| AC7 | `crates/core/src/ui/browse/tests.rs` :: `ac7_mix_rows` (table) | exact command per row; popup | stub `Enter` on a mix sends `LoadQueue` |
| AC8 | `crates/core/src/ui/browse/tests.rs` :: `ac8_go_to_radio` (table) | position in the popup; `r` per row kind; page and request; no command | stub popup omits *Go to radio* and `r` does nothing |
| AC9 | `crates/core/src/ui/keymap.rs` :: `ac9_mixes_and_radio_keys` + 0008's help and unsupported-action tests (rows updated) | default keys, action parsing, unbinding, help text | stub keeps `GoToRadio` unsupported and binds no `r` |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_disconnected` (table) | nothing sent; one re-fetch | stub requests while disconnected |
| AC11 | `crates/app/src/ui.rs` :: `ac11_mixes_80x24` (one snapshot per row), `ac11_mixes_narrow_50x20`, `ac17_no_panic_any_size` (new pages) | `contains` + snapshots | stub render draws the queue |
| AC12 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (a `g m` step) | answered, no session | stub client drops the request |
| AC13 | — reviewed at acceptance | docs match this spec, linked, zh-TW in step | — |

Checked by hand at acceptance with a real account (results in the PR description): `g m` lists the same mixes as the Tidal app's *My Mixes* (video mixes aside); a mix page shows the same tracks in the same order; `Enter` on a track plays it with the rest of the mix after it; `r` on a track and on an artist, and *Go to radio* on the playing track, open a radio that looks related.

## Crate placement

- `tidal-player-core`: `library` (`MixSummary`, `RadioSeed`, the new requests and pages), `ui` (the pages, the mix row, *Go to radio*), `ui::keymap` (`MixesPage`, the default `GoToRadio` binding). No I/O, no new dependency
- `tidal-player-api::library`: the mixes list, mix page and radio calls and their DTO mapping
- `tidal-player`: `player_runtime` (the `Library` trait answers the new requests), `ui.rs`/`ui/pages.rs` (the three pages)
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-09, from code and docs):

- 0004's probe (2026-10-07, live API): `GET /v1/tracks/{id}/radio` → bare tracks, default page 10, at most 100, the seed first; `GET /v1/mixes/{TRACK_MIX}/items` → `{item, type: "track"}` entries, default page 100, the same 100 tracks in the same order; unknown track → `404`/`2001` `Track radio cannot be generated for trackId [1]`; unknown mix → `404`/`2001` `Mix items for [...] not found`
- tidalt's v1 calls above worked against the live API with the same client ID as ours (`e253b75`, 2026: 9 mixes, 8 video mixes dropped; tracks with artist and album)
- 0008 lists spotify-player's `GoToRadio` among the skipped actions (`docs/config.md`)
- spotify-player (`spotify_player/src/command.rs` `construct_*_actions`, `src/event/mod.rs` `handle_go_to_radio`, read 2026-10-09): `GoToRadio` is in the actions of a track (after `GoToArtist`, `GoToAlbum`), an album, an artist (first) and a playlist (first); it opens a tracks page titled `<name> Radio` seeded by the item; it has no default key (`src/config/keymap.rs`) and no mixes page
- The current code: `Item` has `Track`, `Album`, `Playlist` and refuses mix links (`crates/core/src/item.rs`); `PageRequest`/`ListRef` as 0006/0007 (`crates/core/src/library.rs`); neither `g m` nor `r` is bound in 0008's defaults, which have no `[[actions]]` (`examples/keymap.toml`: "none by default")

Verified on 2026-10-09 against the **live API** by `scripts/tidal-mixes-probe.sh`, run by the user (country NG; read-only; the script was deleted after this update). Fixtures under `crates/api/tests/fixtures/mixes/` are written from these shapes:

- `GET /v1/pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US` → `200 {id, title: "My Mix", rows: [{modules: [{type: "MIX_LIST", title: "", pagedList: {dataApiPath: "pages/data/<uuid>", limit: 20, offset: 0, totalNumberOfItems: 15, items}, supportsPaging: false, …}]}]}`: one row, one module, all 15 mixes in the first page. Items: `{id, title, subTitle, description, graphic, images {SMALL, MEDIUM, LARGE}, sharingImages, mixType, contentBehavior, shortSubtitle}`; `mixType` `DISCOVERY_MIX` (*My Daily Discovery*, `subTitle` a sentence), `DAILY_MIX` (*My Mix 1*…, `subTitle` `<artist>, <artist>, <artist> and more`, `description` the genres), `VIDEO_DAILY_MIX` (*My Video Mix 2*…). Mix IDs are 30 lower-case hex digits
- Its `dataApiPath` with `limit=10&offset=10` → `200 {limit: 10, offset: 0, totalNumberOfItems: 15, items}` holding the **last 5** mixes: the offset is applied, but the answer reports `offset: 0`
- `GET /v1/pages/mix?mixId={id}&deviceType=BROWSER&locale=en_US` → `{title, rows: [{modules: [{type: "MIX_HEADER", mix: {id, title, subTitle, mixType, mixNumber, images, detailImages, titleColor, …}, playbackControls}]}, {modules: [{type: "TRACK_LIST", pagedList: {dataApiPath: "pages/data/<uuid>?mixId={id}", limit: 10, totalNumberOfItems: 10, items: [bare tracks, with album.releaseDate]}}]}]}`. Unknown mix → `404 {"subStatus":2001,"userMessage":"Not found"}`
- `GET /v1/mixes/{id}` → `404`/`2001` `Resource not found`: no such resource
- `GET /v1/mixes/{id}/items` → `{limit, offset, totalNumberOfItems, items: [{item, type}]}`, `item` the full track of 0004's fixtures (with `artist`, `artists[]`, `album {id, title, cover}`, `audioModes`, `mixes.TRACK_MIX`). **`limit` and `offset` are ignored**: for *My Daily Discovery*, no parameter, `limit` 50/100/200/1000, `offset=50` and `offset=100` all answered `limit: 10, offset: 0, total: 10` with the same 10 tracks. A video mix with `limit=3` answered all 50 items, `type: "video"`, `album: null`. An artist mix with `limit=100` answered 100 of 100. Unknown mix → `404`/`2001` `Mix items for [<id>] not found`
- `GET /v1/tracks/{id}/radio?limit=100` (seed 145060431) → 98 bare tracks, the **seed first**, `totalNumberOfItems: 98`; `limit=50&offset=50` → the last 48 (this endpoint does page)
- `GET /v1/artists/{id}/radio` (artist 35361) → bare tracks, default page 10, `totalNumberOfItems: 100`; `limit=100` → all 100; **`limit=1000` → `400`/`1001` `Too big page, max page size is [100]`**; unknown artist → `404 {"subStatus":2001,"userMessage":"Artist radio cannot be generated for artist [1]"}`. Its first 8 track IDs equal those of the artist's `mixes.ARTIST_MIX` items: the artist radio *is* the artist mix, as 0004 found for tracks
- `GET /v1/artists/{id}` carries `mixes: {ARTIST_MIX: <id>}`; tracks carry `mixes: {TRACK_MIX: <id>}`
- Every sampled mix and radio track was `["STEREO"]`; the Dolby-Atmos drop is kept for safety, as in 0007

Not verified:

- A mix list longer than its first page (the `dataApiPath` walk is tested on a fixture only)
- A mix of more than 100 tracks (none seen; the single request takes whatever Tidal answers)
- Whether the Tidal apps keep the seed at the top of a track radio

## Decisions (answered by the user, 2026-10-09; folded into the body above)

1. **Where mixes live**: *a page of their own, `g m`* ("gm is fine")
2. **Radio**: *a page* ("as a page"), as spotify-player's *Go to radio*; nothing plays until `Enter`
3. **A default key for *Go to radio***: *`r`* ("r can be fine"), tidalt's key, on the selected row
4. **Which radios**: *track and artist only* ("y"); no album, playlist or mix radio
5. **Mix links as items**: *no*. Mix links stay refused by `parse_item`, so `play`, `o`/`O`, `playback load/add` and MPRIS `OpenUri` are unchanged, and there is no `Item::Mix`. As a consequence a mix row cannot be queued whole (`Z` does nothing on it, its popup has *Open* only): open it, then `Enter` on a track queues the whole mix

## Out of scope

- Favoriting mixes and a "favorite mixes" list
- Tidal's home, explore and genre pages, and their editorial mixes beyond *My Mixes*
- Video mixes and videos
- Mixes and radio among search results (Tidal's search has no mix type in the lists 0007 uses)
- Saving a radio or a mix as a playlist
- Album and playlist radio
- Changing autoplay (0004): it keeps the track radio
- One-shot `playback radio …`
- Mix links as items, and queueing a mix whole from its row (decision 5)

## Implementation notes (choices made where the spec was silent, 2026-10-09)

- **API**: `Subject::Mix(id)` renders `Mix <id>`; an internal `Subject::Radio` marks a radio `404`/`2001`, turned into an empty list. The mixes list reads only `MIX_LIST` modules; its `dataApiPath` walk asks the module's own `limit` (or the first page's item count when `0`) at the player's own offset, sends `deviceType`/`locale`, and stops on an empty answer. An empty or all-space `subTitle` is `None`. Track items that cannot be read are skipped, as in search. The track radio's header track is not filtered by audio mode. A body without the expected module is `malformed mixes/mix response from Tidal`
- **Player**: no change: `page_size_for` falls back to `page_size`, unused by these pages
- **Client model**: `PageKind::{Mixes, Mix, TrackRadio, ArtistRadio}`, `Header::{Mix, Radio}`, `WindowKind::{Mixes, MixTracks, RadioTracks, ArtistRadioTracks}` (the last only for its empty message). A window over a whole list has `whole: true` and an inert placeholder `ListRef` that is never sent: `ask_more` returns early and the window is complete once its total is known. Only the mixes page prefixes its failure (`Could not load the mixes: …`); mix and radio pages show the player's message as is. An empty mixes page is titled `Mixes · 0 mixes`. *Go to radio* on the playing track uses its track ID. The default `r` → `GoToRadio` is a `Binding::Action` in `defaults()`; `GoToRadio` follows `GoToArtist` in `ACTIONS`; the keys help gains an "Actions" section
- **TUI**: a mix row is one line, `title  subtitle`, truncated as a whole (rows are styled per line, so the subtitle is not dimmed separately). "At least 8 rows tall" is measured on the page area below the playback window
- **Build**: worktrees sharing one `CARGO_TARGET_DIR` reused each other's test binaries (an audio test binary kept the removed worktree's fixture path); parallel slices need their own target directories

## Bugs

- **Test plan errors, red reasons (found at acceptance, 2026-10-09).** The plan named per-test red failures that the shared stub did not produce. AC2–AC4: the red stub answered every new page with `Malformed("page")`, so each test failed on its first assertion with that error, not on "keeps video mixes", "pages with `offset`" or "fails the page on a radio `404`". AC7: its red failed on the stub `g m` that builds the mix page, before reaching the `Enter`/`Z` rows. AC5 and AC12 have no red: the server tests' fake library answers any request, and the daemon `g m` step passed as soon as slices A and B were in. Both stay as regression guards. The tests were right; this entry corrects the plan

