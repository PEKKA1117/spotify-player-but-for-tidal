# 0011 — Mixes and radio

- **Status**: draft (2026-10-09). API shapes marked "Not verified" wait for `scripts/tidal-mixes-probe.sh`, run by the user
- **Owner**: tech-lead (primary session)
- **Depends on**: 0004 (implemented: items, `Open` expansion, autoplay's track radio), 0006 (implemented: pages, the history, lists that load as you scroll, the actions popup, `Library`/`LibraryReply`), 0008 (implemented: the keymap, `[[actions]]`, the keys help)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Mixes" and "Radio" sections and the new key; [`docs/playback.md`](../playback.md) "Items" gains mix links; [`docs/config.md`](../config.md) gains `MixesPage` and `GoToRadio` (AC15)

## Context

Tidal builds mixes for each user (*My Daily Discovery*, *My New Arrivals*, daily mixes by genre or era) and a radio for any track or artist. So far the player uses them only behind autoplay (0004: the track radio at the end of the queue). This spec makes them browsable, as spotify-player does with its radio page:

- a **mixes page** listing the user's mixes, and a **mix page** for each, a track list like an album's
- **radio pages** for a track or an artist, opened with spotify-player's *Go to radio* action
- **mix links** (`https://tidal.com/mix/<id>`) accepted wherever items are: the command line, `o`/`O`, `playback load/add`, MPRIS `OpenUri`

It adds no new mechanism where 0006 has one: mix and radio pages are pages in the history, fetched by the player, their lists load as you scroll, and their rows are 0006's rows with 0006's keys and actions.

### What tidalt did

Read from tidalt's `internal/tidal/api.go` (`GetMixes`, `GetMixTracks`, `GetTrackRadio`), `internal/ui/{keys,overlay}.go`, `docs/architecture.md` "Daily Mixes" and the commits named:

- **Mixes from a removed API** (`b1eddc8`, `40bf41e`, then `e253b75`): the v2 `openapi.tidal.com/v2/userRecommendations/me/relationships/myMixes` returned bare track IDs, fetched one `/v1/tracks/{id}` per track, then re-sorted. Tidal removed the resource and the view came up empty. `e253b75` moved to v1: `GET /v1/pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US` for the list (every module's `pagedList.items`, duplicates and ID-less entries skipped, `mixType` containing `VIDEO` dropped: 9 mixes kept, 8 video mixes dropped on the author's account), `GET /v1/mixes/{id}/items` for the tracks (full tracks, `{item, type}`, non-`track` dropped)
- **`Enter` on a mix replaced the queue** with its tracks at once: no way to see what a mix holds before playing it, and no choosing a track in it
- **The mix list was fetched once** (at start, and when the section was first opened) and never refreshed, though daily mixes change every day
- **Radio played at once** (`r`, *Start radio from this*): `GET /v1/tracks/{id}/radio?limit=100` replaced the queue, marked `radio · unsaved`. Track radio only: no artist radio
- **Mix links** (`tidal.com/mix/<id>`) were accepted in its search input, loading the mix into the queue

### What could go wrong here, and the criterion that covers it

1. **A removed or changed endpoint** (tidalt: v2 `userRecommendations` → `404`, an empty view with no message) → v1 endpoints from the probe, fixtures from its shapes, and a failed fetch says so in the page (AC3, AC12)
2. **Video mixes and video items queued as tracks** → video mixes are left out of the list; items that are not tracks, or tracks without `STEREO`, are dropped from mix and radio lists (AC3, AC4, AC5)
3. **One request per track** (tidalt's v2 fan-out) → a mix's tracks come as full tracks from its items, a page at a time (AC4)
4. **A mix played before it can be seen** (tidalt `Enter`) → `Enter` on a mix opens its page; only `Enter` on a track plays, with the rest of the mix queued (0006's rule) (AC9)
5. **Stale daily mixes** (tidalt: fetched once) → opening the mixes page always fetches it, as every 0006 page (AC8)
6. **Radio that throws the queue away** (tidalt `r`) → *Go to radio* opens a page; the queue changes only on `Enter` or `Z`, as everywhere (AC10)
7. **A seed with no radio** (`404`, "Track radio cannot be generated", 0004 "Facts") → an empty radio page saying so, not a failure (AC5)
8. **Mix links refused** (0004 refuses them today) → parsed as `Item::Mix` and expanded by the player like a playlist (AC2, AC6)

## Behaviour

### Pages

Three new page kinds, opened and kept in the history as 0006's:

| Page | Opened with | Windows | Title row |
|---|---|---|---|
| Mixes | `g m` (`MixesPage`*), from anywhere | the user's mixes | `Mixes · 9 mixes` |
| Mix | `Enter` on a mix; *Open* in its popup | the mix's tracks | `<title> · <subtitle> · 100 tracks` |
| Radio | *Go to radio* on a track or an artist (actions popup, or `[[actions]] GoToRadio`) | the radio's tracks | `<track title> Radio · <artists>` / `<artist name> Radio` (spotify-player's `<name> Radio`) |

- **Mix rows**: title, then the subtitle (Tidal's, e.g. `Pierce The Veil, Sleeping With Sirens and more`), truncated with `…` as 0006's rows. Tidal's order
- **Track rows** on mix and radio pages: 0006's track rows (non-streamable dimmed)
- Empty: `No mixes yet`, `This mix has no tracks`, `No radio for this track`, `No radio for this artist`. Failures in the page: `Could not load the mixes: <reason>`, `Mix <id> was not found`, `Track 1 was not found`, `Artist 1 was not found`
- Every page fetches fresh when opened (0006, 0009 decision 3); going back shows it as it was

### Lists load as you scroll

0006's rules unchanged, with `page_size` (0006, `app.toml` since 0008), clamped to each endpoint's largest page (table below). The whole list is fetched first for `Enter` on a track, `Z` and *Add to playlist…* of a list, as in 0006. A radio is a single list of at most 100 tracks (Tidal's cap, 0004 "Facts"), so it never asks for a second page.

### Playing, queueing and actions

0006's tables hold, with mixes as a new row kind:

| Key | On | Does |
|---|---|---|
| `Enter` | a mix | Opens its page |
| `Enter` | a track on a mix or radio page | `LoadQueue { every track of the list, start: index }` (0006: the rest fetched first) |
| `Z`, `C-z` | a mix | `Open { items: [Mix(id)], at: Some(End) }` (the player expands it) |

| Item | Actions, in this order |
|---|---|
| Mix | *Open*, *Add to queue*, *Play next* |
| Track (browse page, queue entry, playing track) | 0006's list with ***Go to radio*** after the *Go to artist* entries |
| Artist | *Open*, ***Go to radio***, *Add to favorites*/*Remove from favorites* |

- *Go to radio* on a track opens `Radio(track)`; on an artist `Radio(artist)`. It is spotify-player's `GoToRadio` action, so `[[actions]] action = "GoToRadio"` binds it (it was in 0008's skipped list). **No default key** (spotify-player has none; decision 3)
- Favoriting mixes is out of scope: a mix row's popup has no favorite entries

### Mix links

`parse_item` (0004 AC16) accepts mix links: `https://tidal.com/mix/<id>`, `…/browse/mix/<id>`, `listen.tidal.com`/`www.` hosts, `tidal://mix/<id>`, with 0004's trailing slash, `/u`, query and fragment rules. A mix ID is 1–64 ASCII letters and digits (the probe records the real form). It becomes `Item::Mix(String)` (`Mix <id>` in messages). The refusal message becomes `Not a Tidal track, album, playlist or mix: <item>`; artist and video links stay refused.

The player expands `Mix(id)` as it expands a playlist (0004 "Filling the queue"): every track of `GET /mixes/{id}/items`, in order, paged to the end, non-tracks and tracks without `STEREO` dropped. A mix with no tracks left is refused: `Mix <id> has no tracks`; an unknown one: `Mix <id> was not found`. So `tidal-player play <mix link>`, `tidal-player <mix link>`, `playback load|add <mix link>`, `o`/`O` and MPRIS `OpenUri` all take mix links with no further change.

### Talking to the player

0006's `Library` request, additions:

- `PageRequest::Mixes` → `PageData::Mixes { mixes: ListPage<MixSummary> }`
- `PageRequest::Mix(String)` → `PageData::Mix { mix: MixSummary, tracks: ListPage<Track> }`
- `PageRequest::TrackRadio(u64)` → `PageData::Radio { seed: RadioSeed::Track(Track), tracks: ListPage<Track> }`; `PageRequest::ArtistRadio(u64)` → `PageData::Radio { seed: RadioSeed::Artist(ArtistRef), tracks }`
- `ListRef::Mixes`, `MixTracks(String)`, `TrackRadio(u64)`, `ArtistRadio(u64)` for `More`
- `MixSummary { id: String, title, subtitle: Option<String>, kind: Option<String> }` (`kind` is Tidal's `mixType`, kept for display-free filtering and tests)

Errors are 0006's wording; a `404`/`2001` on a mix is `Mix <id> was not found`.

### What the player fetches

`countryCode` on every call, as everywhere. Shapes are tidalt's and 0004's probe; the rows marked † wait for this spec's probe.

| Request | API call | Largest page | Kept |
|---|---|---|---|
| `Page(Mixes)`, `More { Mixes }` | `GET /pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US`; later pages from the module's `dataApiPath` if it has one † (as 0006's credits) | † | every module's `pagedList.items[]`: `id`, `title`, `subTitle`, `mixType`; `mixType` containing `VIDEO` dropped; a repeated or empty `id` dropped |
| `Page(Mix(id))` header | `GET /pages/mix?mixId={id}&deviceType=BROWSER&locale=en_US` † (its `MIX_HEADER` module: `title`, `subTitle`) | — | `MixSummary` |
| `Page(Mix(id))` tracks, `More { MixTracks }`, `Open` expansion | `GET /mixes/{id}/items?limit=…&offset=…` | 100 † | `items[]` with `type == "track"`: `item` (0004's track mapping), without `STEREO` dropped |
| `Page(TrackRadio(id))` | `GET /tracks/{id}` (0004) and `GET /tracks/{id}/radio?limit=100` (0004 AC27) | 100 | the header track; bare tracks, without `STEREO` dropped. The seed is the first radio track and **is kept** (it is the page's subject, as in the Tidal apps †) |
| `Page(ArtistRadio(id))` | `GET /artists/{id}` (0006) and `GET /artists/{id}/radio?limit=100` † | 100 † | the header artist; bare tracks, without `STEREO` dropped |

- A header `404`/`2001` fails the page (`Mix <id> was not found`, 0006's track and artist messages). A **radio** `404`/`2001` is an empty list (`No radio for this track`), as autoplay already treats it (0004 AC27)
- A page's header and first list page are fetched in parallel; any failing call fails the page (0006)
- Autoplay (0004) is unchanged: it keeps calling the track radio itself

### Keys

| Command | Default keys | Acts in | Help text |
|---|---|---|---|
| `MixesPage`* | `g m` | everywhere | your mixes |

`g m` is no binding's prefix and starts none (0008's conflict check stays clean). Action for `[[actions]]`: `GoToRadio` (tracks, artists; target `SelectedItem` or `PlayingTrack`), removed from 0008's list of skipped spotify-player actions.

### Rendering

As 0006: one window per page, the title row above it, `Loading…`, `Loading more…`, failures and empty messages in the window. Nothing panics at any size.

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Mixes · 9 mixes                                                               │
│┌Mixes (9)────────────────────────────────────────────────────────────────────┐│
││My Daily Discovery     Pierce The Veil, Sleeping With Sirens and more         ││
││My Mix 1               Dean Lewis, Lewis Capaldi and more                     ││
│…                                                                             │
```

## Acceptance criteria

Protocol and types (`tidal_player_core`, pure):

- **AC1** — `MixSummary`, `RadioSeed`, the new `PageRequest`, `PageData` and `ListRef` variants and `Item::Mix` survive a JSON round trip and 0005's codec (0001 AC11's and 0005 AC2's tests, new rows)
- **AC2** — `parse_item` (0004 AC16's table, new rows): mix links on every accepted host and scheme, with `/browse`, a trailing slash, `/u`, a query and a fragment → `Item::Mix(id)`; an empty ID, one with `-` or `/`, and one of 65 characters refused; artist and video links still refused, with the new message

API (`tidal-player-api::library`, wiremock fixtures from the probe):

- **AC3** — `Page(Mixes)`: one `GET /pages/my_collection_my_mixes` with `deviceType`, `locale`, `countryCode`; mixes from every module in order; video mix types, repeated and empty IDs dropped; `More { Mixes }` per the probe's paging (or no request when the first page held the total); `404`, `LoginRequired` and transient errors as 0006 (table)
- **AC4** — `Page(Mix(id))`: the header from `/pages/mix` and the first tracks page from `/mixes/{id}/items` in parallel; `More { MixTracks }` with `limit = min(asked, 100)` and `offset`; non-track items and tracks without `STEREO` dropped; `404`/`2001` → `Mix <id> was not found`; any failing call fails the page (table)
- **AC5** — Radio: `TrackRadio(id)` sends `GET /tracks/{id}` and `GET /tracks/{id}/radio?limit=100`, `ArtistRadio(id)` `GET /artists/{id}` and `GET /artists/{id}/radio?limit=100`; the seed track kept first; tracks without `STEREO` dropped; a radio `404`/`2001` → an empty list; a header `404` → `NotFound` (table)
- **AC6** — `Open` expansion of `Item::Mix`: every page of `/mixes/{id}/items` until the total, in order, filtered as AC4; no track left → `Mix <id> has no tracks`; `404` → `Mix <id> was not found` (beside 0004 AC17's album and playlist walks)

Player (`tidal-player`):

- **AC7** — The new `Library` requests are answered like any 0006 request: one `LibraryReply` to the sender, beside the input loop (0006 AC8's harness, new rows)

Client model (`tidal_player_core::ui`, pure):

- **AC8** — `g m` pushes the mixes page and emits `Library { Page(Mixes) }` (on top already: nothing); `Enter` on a mix pushes its page and emits `Page(Mix(id))`; going back shows kept rows with no request; scrolling emits `More { Mixes }` / `More { MixTracks(id) }` per 0006 AC10 (table)
- **AC9** — Rows (table): `Enter` on track *i* of a mix or radio page sends `LoadQueue { the whole list, start: i }` (fetched first when partly loaded); `Z` on a mix sends `Open { [Mix(id)], Some(End) }`; a mix's popup lists *Open*, *Add to queue*, *Play next* and runs them (*Play next*: `at: Next`)
- **AC10** — *Go to radio* (table): listed after the *Go to artist* entries for a browse track, a queue entry and the playing track (`a`), and after *Open* for an artist; running it pushes `Radio` and emits `Page(TrackRadio(id))` / `Page(ArtistRadio(id))`; it changes no queue and sends no command
- **AC11** — Keymap (0008's tables, new rows): `MixesPage` defaults to `g m` and is listed in the keys help as `your mixes`; `[[actions]] action = "GoToRadio"` parses with both targets and runs on a track and an artist, and does nothing on an album, playlist or mix; `GoToRadio` is no longer reported as an unsupported spotify-player action; the default keymap has no prefix conflict
- **AC12** — Disconnected and session expired: 0006 AC16's rows for the three new pages (failed with the disconnected message, re-fetched on the next `Welcome` when on top)

TUI rendering and runtime (`tidal-player`):

- **AC13** — Rendering (`insta`, 80×24, reviewed by eye, plus `contains` checks): mixes page loaded (title with count, subtitles); mix page; track radio page; artist radio page; `No mixes yet`; `No radio for this track`; a failed mixes page; the actions popup on a track showing *Go to radio*. 50×20: the mix page. No panic from 0×0 to 120×40 on the three pages
- **AC14** — Wiring: an attached client's `g m` is answered from the test player's wiremock API and opens no session store (0006 AC19's test, a `g m` step); `tidal-player play <mix link>` against wiremock queues the mix's tracks (0004's `play` harness)

Docs:

- **AC15** — `docs/tui.md` documents the mixes, mix and radio pages, `g m`, *Go to radio* and the messages; `docs/playback.md` "Items" lists mix links; `docs/config.md` lists `MixesPage` and `GoToRadio` and drops `GoToRadio` from the unsupported list; `examples/keymap.toml` shows `g m`; the zh-TW copies and `README.md`/`README.zh-TW.md` match; `CLAUDE.md` "Features" has the entry; this spec links to them

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| A new account with no mixes | `No mixes yet` |
| Only video mixes | Left out: `No mixes yet` |
| A mix link to a video mix | `Mix <id> has no tracks` (its items are videos) |
| A mix link that does not exist | `Mix <id> was not found`: exit 1 for `play`, the message in the TUI |
| A track Tidal has no radio for | `No radio for this track` in the page; the queue is untouched |
| *Go to radio* on a track that does not stream | The page opens (the radio may still exist) |
| A radio track that is the seed | Kept first, so `Enter` on it plays the seed with its radio after it |
| Daily mixes changed since the page was opened | Going back shows the old rows (0006's history rule); `g m` from another page fetches fresh |
| `429` | The page or the `Loading more…` row says `Tidal answered 429: try again in a moment`; nothing retries |
| Session expired, disconnected | 0006's rules (AC12) |
| Narrow terminal | One window, as 0006 (AC13) |

## Test plan

Each test is named after its criterion (`ac4_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0010. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/mixes/`, written from the probe's recorded shapes (user IDs replaced).

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | stub `Serialize` writes `null` |
| AC2 | `crates/core/src/item.rs` :: `ac16_parse_item` (new rows) | `Item::Mix` per form; refusals and message | stub refuses every mix link |
| AC3 | `crates/api/tests/mixes.rs` :: `ac3_mixes_list` (table) | path, params, filtering, paging, errors | stub keeps video mixes |
| AC4 | `crates/api/tests/mixes.rs` :: `ac4_mix_page` (table) | header, items, clamp, drops, `NotFound` | stub keeps non-track items |
| AC5 | `crates/api/tests/mixes.rs` :: `ac5_radio` (table) | paths, seed kept, `404` → empty | stub fails the page on a radio `404` |
| AC6 | `crates/api/tests/mixes.rs` :: `ac6_mix_expansion` (table) | full walk, drops, messages | stub reads the first page only |
| AC7 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (new rows) | one reply to the sender | stub answers the new requests with an error |
| AC8 | `crates/core/src/ui/browse/tests.rs` :: `ac8_mixes_pages` (table) | push, request, kept state, `More` | stub `g m` does nothing |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_mix_rows` (table) | exact command per row; popup | stub `Enter` on a mix sends `LoadQueue` |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_go_to_radio` (table) | position in the popup; page and request; no command | stub popup omits *Go to radio* |
| AC11 | `crates/core/src/ui/keymap.rs` :: `ac11_mixes_and_radio_keys` + 0008's help and unsupported-action tests (rows updated) | default key, action parsing, help text | stub keeps `GoToRadio` unsupported |
| AC12 | `crates/core/src/ui/browse/tests.rs` :: `ac12_disconnected` (table) | nothing sent; one re-fetch | stub requests while disconnected |
| AC13 | `crates/app/src/ui.rs` :: `ac13_mixes_80x24` (one snapshot per row), `ac13_mixes_narrow_50x20`, `ac17_no_panic_any_size` (new pages) | `contains` + snapshots | stub render draws the queue |
| AC14 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (a `g m` step) + `crates/app/src/play.rs` :: `ac14_play_mix_link` | answered, no session; mix queued | stub client drops the request; stub refuses the link |
| AC15 | — reviewed at acceptance | docs match this spec, linked, zh-TW in step | — |

Checked by hand at acceptance with a real account (results in the PR description): `g m` lists the same mixes as the Tidal app's *My Mixes* (video mixes aside); a mix page shows the same tracks in the same order; `Enter` on a track plays it with the rest of the mix after it; *Go to radio* on the playing track and on an artist opens a radio that looks related; `tidal-player play https://tidal.com/mix/<id>` plays a mix.

## Crate placement

- `tidal-player-core`: `item` (`Item::Mix`, parsing), `library` (`MixSummary`, `RadioSeed`, the new requests, pages and list refs), `ui` (the pages, the mix row, *Go to radio*), `ui::keymap` (`MixesPage`, `GoToRadio`). No I/O, no new dependency
- `tidal-player-api::library`: the mixes list, mix page and radio calls and their DTO mapping; `metadata` (the `Open` expansion of a mix)
- `tidal-player`: `player_runtime` (the `Library` trait answers the new requests; `Open` expands mixes), `ui.rs`/`ui/pages.rs` (the three pages)
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-09, from code and docs):

- 0004's probe (2026-10-07, live API): `GET /v1/tracks/{id}/radio` → bare tracks, default page 10, at most 100, the seed first; `GET /v1/mixes/{TRACK_MIX}/items` → `{item, type: "track"}` entries, default page 100, the same 100 tracks in the same order; unknown track → `404`/`2001` `Track radio cannot be generated for trackId [1]`; unknown mix → `404`/`2001` `Mix items for [...] not found`
- 0006's probe: `GET /v1/artists/{id}` carries a `mixes` object (the artist's mix IDs)
- tidalt's v1 calls above worked against the live API with the same client ID as ours (`e253b75`, 2026: 9 mixes, 8 video mixes dropped; tracks with artist and album)
- 0008 lists spotify-player's `GoToRadio` among the skipped actions (`docs/config.md`)
- spotify-player (`spotify_player/src/command.rs` `construct_*_actions`, `src/event/mod.rs` `handle_go_to_radio`, read 2026-10-09): `GoToRadio` is in the actions of a track (after `GoToArtist`, `GoToAlbum`), an album, an artist (first) and a playlist (first); it opens a tracks page titled `<name> Radio` seeded by the item; it has no default key (`src/config/keymap.rs`) and no mixes page
- The current code: `Item` has `Track`, `Album`, `Playlist` (`crates/core/src/item.rs`); `PageRequest`/`ListRef` as 0006/0007 (`crates/core/src/library.rs`); no `g m` binding in 0008's defaults

Not verified (the probe `scripts/tidal-mixes-probe.sh` answers them; the spec is updated before approval):

- The `my_collection_my_mixes` page's shape today: modules, `pagedList` totals, whether a `dataApiPath` pages it, the `mixType` values and the mix ID form
- `GET /v1/pages/mix?mixId=…` as the mix header (title, subtitle), and its answer for an unknown mix; `GET /v1/mixes/{id}` as an alternative
- The largest page of `/mixes/{id}/items` and how deep `offset` goes; whether a daily mix holds more than 100 tracks
- `GET /v1/artists/{id}/radio`: its existence, envelope, cap and `404`; whether it matches the artist's `ARTIST_MIX`
- Whether the Tidal apps keep the seed at the top of a track radio

## Decisions (open: answer before approval)

1. **Where mixes live**: a page of their own, `g m` (proposed: tidalt's key, free in 0008's defaults), or a fourth window on the library page?
2. **Radio**: a page, as spotify-player's *Go to radio* (proposed), or play at once replacing the queue, as tidalt's `r`?
3. **A default key for *Go to radio***: none, popup only (proposed, spotify-player's default), or `r` (tidalt)?
4. **Which radios**: track and artist (proposed). spotify-player also offers album and playlist radio; Tidal has no such endpoint known to us, so they would need a seed choice (e.g. the album's first track's radio). Leave them out?
5. **Mix links as items** (`play`, `o`/`O`, `playback load/add`, `OpenUri`): proposed yes, changing 0004's refusal

## Out of scope

- Favoriting mixes and a "favorite mixes" list
- Tidal's home, explore and genre pages, and their editorial mixes beyond *My Mixes*
- Video mixes and videos
- Mixes and radio among search results (Tidal's search has no mix type in the lists 0007 uses)
- Saving a radio or a mix as a playlist (*Add to playlist…* on its tracks already copies them)
- Album and playlist radio
- Changing autoplay (0004): it keeps the track radio
- One-shot `playback radio …`
