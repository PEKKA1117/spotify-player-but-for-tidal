# 0024 — Browse

- **Status**: approved (2026-11-05)
- **Owner**: tech-lead (primary session)
- **Issue**: [#69](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/69)
- **Depends on**: 0006 (implemented: pages, the history, lists that load as you scroll, the actions popup, tabs of a pane), 0008 (implemented: the keymap, the keys help), 0011 (implemented: mix pages, whole lists), 0012 (implemented: the filter), 0020 (implemented: the selection)
- **User docs**: [`docs/tui.md`](../tui.md) gains a "Browse" section and the new key; [`docs/config.md`](../config.md) gains `BrowsePage` and loses it from the skipped commands; and `hide_browse_pages`; `examples/app.toml`; their zh-TW copies, `README.md` and its zh-TW copy (AC15)

## Context

So far the player shows what the user already has (library, mixes) or asks for (search). Tidal also offers what it picks: a personal home, editorial lists, new releases, genres, moods, decades. spotify-player has this as its `BrowsePage` on `g b`, and 0011 left Tidal's home, explore and genre pages out of scope.

This spec adds a **browse page**, opened with `g b`, and the pages it leads to. It adds no new mechanism where 0006 has one: a browse page is a page in the history, fetched by the player. Its sections are the tabs of one pane (the artist page's mechanism), its lists load as you scroll (0006) or arrive whole (0011), and its rows are 0006's rows with 0006's keys and actions. What is new is a row that leads to another browse page, and a list that can hold rows of several kinds.

Two sources feed it, because each holds what the other lacks (see "Facts"):

- **v1 `pages/*`**: editorial pages, the same for every user, with a stable shape and real paging
- **v2 `home/feed/static`**: the personal home of Tidal's own apps (recently played, recommendations), undocumented and with no paging

### What could go wrong here, and the criterion that covers it

1. **A page with many sections does not fit** (12 lists on a genre page, 17 on the home) → one pane of tabs, and a tab strip that scrolls with the active tab (AC9, AC13)
2. **Videos queued as tracks** → `VIDEO_LIST` modules, video mixes and video-only playlists never become rows (AC3, AC6)
3. **A module type Tidal adds tomorrow fails the page** → unknown modules, and items that cannot be read, are skipped, never an error (AC2, AC6)
4. **The undocumented v2 feed changes or goes away** → a failed home says so in its own page; every v1 page keeps working (AC12)
5. **Paging a list that does not page**: `view-all` honours `limit` and `offset` for some modules and ignores them for others ("Facts"), so paging it would repeat or lose rows → it is asked once, with neither, and kept whole (AC7)
6. **Rows of several kinds in one list**: `Enter` on a track queues the rest of the list, which also holds albums and mixes → only the tracks are queued (AC10)
7. **A path from a client reaching any endpoint**: paths travel from the page data back to the player through `Command`s → the player only follows paths of the two known forms (AC8)
8. **A dropped row shifting the paging**: rows the player leaves out must not move the next `offset` → `next_offset` advances by the page size asked, as it already does (AC4)

## Behaviour

### The root page

`g b` (`BrowsePage`) opens the root page, from `pages/explore`. It is one pane of four tabs:

| Tab | Rows |
|-----|------|
| *Discover* | *Home*, *Staff picks*, *For you*, then the links of explore's untitled module (*New*, *Top*, *HiRes*, *Clean Content*) |
| *Genres* | explore's *Genres* module (Hip-Hop, Pop, …) |
| *Moods & Activities* | explore's module of that name |
| *Decades* | explore's *Decades* module |

*Home* is the v2 personal feed, *Staff picks* is `pages/home` (the name the v2 header gives the editorial content: Tidal's own title for that page, *Home*, would clash) and *For you* is `pages/for_you`. The tab titles other than *Discover* are Tidal's, so they follow its wording.

Every row of the root page is a **link**: `Enter` opens the page it names.

### Browse pages

A link opens a **browse page**: its title is the page's (`Hip-Hop`, `Top`, `Staff picks`), and each section is a tab of its single pane, in Tidal's order. `[` and `]` switch tabs; a page with one section shows no tab strip.

- A section's tab is the module's title. An untitled module is called by what it holds: `Tracks`, `Albums`, `Playlists`, `Artists`, `Mixes`, or `More` for links
- Rows are drawn as on any other page: a track as in a playlist, an album as in the library, and so on
- A section with **rows of several kinds** (only the v2 feed has these) puts the kind first on each row: `Album`, `Artist`, `Mix`, `Playlist`, `Track`
- The tab strip shows as many tabs as fit, always with the active one, and `…` on a side with more
- Below 60 columns the strip follows 0006's rule for the artist page: the active tab's name, with `[ ]`

### Which rows are kept

Sections, v1:

| Module | Becomes |
|--------|---------|
| `TRACK_LIST`, `ALBUM_LIST`, `PLAYLIST_LIST`, `ARTIST_LIST`, `MIX_LIST` | a section of those rows |
| `PAGE_LINKS`, `PAGE_LINKS_CLOUD` | a section of links |
| `VIDEO_LIST`, `FEATURED_PROMOTIONS`, `MULTIPLE_TOP_PROMOTIONS`, anything else | nothing |

Sections, v2: every module of `items` becomes a section, whatever its `type`; the kind of each of its items decides what is kept (`TRACK`, `ALBUM`, `PLAYLIST`, `ARTIST`, `MIX`; anything else, such as `DEEP_LINK`, is dropped).

Rows left out, in both:

- A track without `STEREO` (0011's rule)
- A mix whose type contains `VIDEO` (0011's rule; `mixType` in v1, `type` in v2)
- A playlist with `numberOfTracks` of 0 (the video playlists: tracks 0, videos 59 and up)
- A link with an empty title

A section left with no rows is dropped. A page left with no sections says `Nothing to show here`.

### Hiding pages

`hide_browse_pages` in `app.toml` lists the browse pages whose links the TUI does not show, by the name after `pages/`:

```toml
hide_browse_pages = ["videos", "tidal_magazine"]   # the default
```

- The default holds the two pages known to have nothing to show: after the rules above `pages/videos` and `pages/tidal_magazine` are empty ("Facts")
- `[]` hides nothing: those links come back, and open on `Nothing to show here`
- A name matches a link's page exactly, case included; a name no link has does nothing. Only links are hidden: a page reached some other way still opens
- The environment variable is `TIDAL_PLAYER_HIDE_BROWSE_PAGES`, in the form of `TIDAL_PLAYER_HIDE_VERSIONS`
- The TUI reads it, as a choice of what to show: the player still answers every link, each client applies its own list, and a section all of whose links are hidden is dropped

### Lists

- **v1 sections** hold their first rows in the page's response. A section with fewer rows than its total asks for more as you scroll, from its `dataApiPath`, exactly as 0006's lists. A section without a `dataApiPath` is whole
- **v2 sections** hold a preview (up to 15 rows). A section with a `viewAll` fetches it the first time its tab is shown, and the whole list replaces the preview: the cursor stays where it is, as the preview is the start of the list. Until it arrives the tab's title ends in `…`. If it fails the preview stays and the message row says so
- A section of either kind is searched by `/` as any list; a v1 section fetches more pages until enough rows match (0012)

### Keys

Everything is 0006's and 0011's: `Enter` opens an album, playlist, artist or mix page; `Enter` on a track plays it with the tracks of the list after it; `Z`, the actions popup, the selection and `g a` work on a row as anywhere else.

| Row | `Enter` | `Z`, actions, selection |
|-----|---------|-------------------------|
| Track, album, playlist, artist, mix | as on every page | as on every page |
| Link | opens the browse page | nothing: a link has no actions and cannot be selected |

`Backspace` goes back with the tab and cursor as they were (0006's history rule); `g b` from another page fetches the root afresh.

In a section of several kinds, `Enter` on a track queues only the tracks of the section; the other rows are skipped.

### Talking to the player

0006's `Library` request, additions:

- `BrowseRef`: `Root` (`pages/explore`), `Home` (the v2 feed), `Page(String)` (a `pages/…` path)
- `PageRequest::Browse(BrowseRef)` → `PageData::Browse { title: String, sections: Vec<BrowseSection> }`
- `BrowseSection { title: String, rows: ListItems, more: BrowseMore }`: 0006's `ListItems`, which gains `Mixes(ListPage<MixSummary>)`, `Links(ListPage<PageLink>)` and `Mixed(ListPage<MixedRow>)`
- `BrowseMore`: `None` (whole), `Data(String)` (a `dataApiPath`: paged), `ViewAll(String)` (a `viewAll` path: one fetch)
- `MixedRow`: `Track(Track)`, `Album(AlbumSummary)`, `Playlist(PlaylistSummary)`, `Artist(ArtistRef)`, `Mix(MixSummary)`: one row of a section of several kinds, the owned form of 0006's `Row` (not `crate::Item`, which names a track, album or playlist by ID for links and commands)
- `PageLink { title: String, target: BrowseRef }`
- `ListRef::BrowseData(String)`, `ListRef::BrowseViewAll(String)`, both answered as `ListItems`

A section of one kind is that kind's list, and only a section that really holds several kinds is `Mixed`. The player decides by the rows it kept, per answer: `view-all` may hold more kinds than its preview (*Recently played*: `ALBUM` and `DEEP_LINK` appear only there), so the answer replaces the window's rows whatever their kind.

UI model: `PageKind::Browse(BrowseRef)`, `WindowKind::BrowseSection`, `Rows::Links(Vec<PageLink>)`, `Rows::Mixed(Vec<MixedRow>)`, `Row::Link`. A section of tracks is `Rows::Tracks` like any other track list, so what already works on those rows works here unchanged. `Rows::get` turns a `MixedRow` into the `Row` of its kind, so everything that goes through `Row` needs nothing new; what matches `Rows::Tracks` directly and must also see a mixed section's tracks is covered by AC10. A `Window` gets its title from the section rather than from its kind.

### Requests

v1, as 0011 (`countryCode`, `deviceType=BROWSER`, `locale=en_US`): `GET /v1/pages/explore`, `/v1/pages/<name>`; `GET /v1/<dataApiPath>` with `limit` and `offset`, where `limit` is the page size capped at 50.

v2 (`countryCode`, `deviceType=BROWSER`, `locale=en_US`, `platform=WEB`, header `x-tidal-client-version: <the version of tidal-player-api>`): `GET /v2/home/feed/static`, then with `cursor` while the answer has one, at most 5 requests; `GET /v2/<viewAll>` with no `limit` or `offset`. A `viewAll` path may carry its own query (`?itemId=…&rankingToken=…`), kept as it is.

## Acceptance criteria

1. **AC1 Protocol**: `BrowseRef`, `MixedRow`, `BrowseSection`, `BrowseMore`, `PageLink`, the new `ListItems` variants, `PageRequest::Browse`, `PageData::Browse`, `ListRef::BrowseData` and `ListRef::BrowseViewAll` round-trip through serde and the socket's framing
2. **AC2 v1 sections**: a page's modules become sections in Tidal's order per the table under "Which rows are kept"; `VIDEO_LIST`, both promotion modules and an unknown module type are skipped, and an item that cannot be read is dropped, without failing the page
3. **AC3 Rows left out**: tracks without `STEREO`, video mixes, playlists with 0 tracks and links with an empty title are left out, in v1 and v2; a section with no rows left is dropped; an untitled module takes its kind's name
4. **AC4 v1 paging**: a section with fewer rows than its total asks `dataApiPath` with `limit` (at most 50) and `offset` as you scroll, the path's own query kept; left-out rows do not change the next `offset`, and a section without a `dataApiPath` never asks
5. **AC5 Root page**: `g b` requests `BrowseRef::Root` and shows the four tabs, *Discover* holding *Home*, *Staff picks*, *For you* and explore's untitled links; explore's promotions are not shown
6. **AC6 v2 feed**: the feed is asked with its parameters and the `x-tidal-client-version` header, follows `cursor` until none, at most 5 requests; every module becomes a section, its `TRACK`, `ALBUM`, `PLAYLIST`, `ARTIST` and `MIX` items read from `data` (a mix's title and subtitle from `titleTextInfo.text` and `subtitleTextInfo.text`), anything else dropped
7. **AC7 view-all**: the first time a v2 section's tab is shown, its `viewAll` is asked once, with neither `limit` nor `offset`; the list replaces the preview whatever its kinds (a one-kind preview may become `Mixed`), keeps the cursor and is never asked again; a failed fetch keeps the preview and puts the message on the message row
8. **AC8 Paths**: the player sends a path only if it starts with `pages/` (v1) or `home/pages/` (v2) and has no `..` segment; anything else answers `Not a Tidal page: <path>` and sends no request
9. **AC9 Navigation**: `Enter` on a link pushes a browse page and requests it; `[` and `]` move between its tabs, wrapping; `Enter` on an album, playlist, artist or mix opens that page; `Backspace` restores the tab and cursor without a fetch; `g b` from another page fetches the root
10. **AC10 Rows and actions**: `Enter` on a track plays it with the list's following tracks queued, skipping rows of other kinds; `Z`, the actions popup and the selection work on rows as on any page; on a link they do nothing, and marking a range skips links
11. **AC11 Filter**: `/` filters the shown tab over each row's fields (a link by its title); a filter belongs to its tab, and a v1 section loads more pages until enough rows match
12. **AC12 Failures**: a failed page shows the failure in the page (`Page <path> was not found` on a `404`, 0006's wording otherwise); a failed *Home* says `Home is not available: Tidal answered <status>` and leaves the rest of browse usable; a page with no sections says `Nothing to show here`
13. **AC13 Rendering**: at 150 × 42, 80 × 24 and 50 × 20: the title, the tab strip with `…` on a side with more and the active tab always in view, no strip for one section, the kind column only for a section of several kinds, a 117-character section title cut to fit
14. **AC14 Keymap**: `BrowsePage` is a command on `g b`, listed in the keys help and key hints; it is no longer among the skipped spotify-player commands, so a `keymap.toml` binding it works
15. **AC15 Docs**: `docs/tui.md`, `docs/config.md`, `README.md` and their zh-TW copies describe the page, its key, its sources and `hide_browse_pages`; `examples/app.toml` shows the setting; 0008's example of a skipped command no longer names `BrowsePage`
16. **AC16 Hiding pages**: `hide_browse_pages` defaults to `["videos", "tidal_magazine"]`, and `app.toml` and `TIDAL_PLAYER_HIDE_BROWSE_PAGES` set it (the environment winning, a value that is not an array of strings refused as `hide_versions`'s is); links to a listed page are not shown, on the root and on every browse page, a section left with none is dropped, and `[]` shows them all

## Edge cases & errors

| Case | What happens |
|------|--------------|
| A page with one section | No tab strip: the page looks like a list |
| A page with only skipped modules | `Nothing to show here` |
| `hide_browse_pages = []` | The *Videos* and *TIDAL Magazine* links show, and open on `Nothing to show here` |
| `hide_browse_pages` edited while the TUI runs | Read at start, as every setting: it takes effect on the next run |
| `v2` answers `400` | *Home* says `Home is not available: Tidal answered 400`; *Staff picks*, *For you* and the rest work |
| A `viewAll` with a `rankingToken` that has expired on a page kept in the history | The fetch fails, the preview stays, the message row says so; `g b` gets a fresh page |
| A section title of 117 characters | Cut to fit the strip, whole in the window's title when focused |
| Two sections with the same title (*Custom mixes* on *For you*) | Both kept: they are different lists |
| A row repeated in a section across pages | Kept once (0011's rule for mixes, by the row's key) |
| A mix that will not open (a `HISTORY_*` mix, "Facts") | 0011's page failure; the browse page is untouched |
| A paid album (*Upload for purchase*) | Listed as Tidal lists it; playback says what it says for any track that does not stream |
| Tidal's rows change while a page is in the history | Going back shows the old rows (0006's rule); opening it again fetches |
| `429` | The page says `Tidal answered 429: try again in a moment`; nothing retries |
| Session expired, disconnected | 0006's rules |
| Empty filter result | 0012's rules |

## Test plan

Each test is named after its criterion (`ac4_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0023. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/browse/`, written from the shapes under "Facts": public catalogue data only; user IDs, recently played rows and image URLs replaced or dropped.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | stub `Serialize` writes `null` |
| AC2 | `crates/api/tests/browse.rs` :: `ac2_v1_sections` (table) | module → section, order, skipped and unknown types, an unreadable item | stub keeps every module |
| AC3 | `crates/api/tests/browse.rs` :: `ac3_rows_left_out` (table) | each left-out row, in v1 and v2; dropped section; title fallback | stub keeps video rows |
| AC4 | `crates/api/tests/browse.rs` :: `ac4_v1_paging` (table) + `crates/core/src/ui/browse/tests.rs` :: `ac4_section_scroll` | `limit`, `offset`, a path with a query; `next_offset` after dropped rows; no ask without a path | stub drops the path's query |
| AC5 | `crates/api/tests/browse.rs` :: `ac5_root` + `crates/core/src/ui/browse/tests.rs` :: `ac5_root_page` | the four tabs and *Discover*'s rows; `g b` sends the request | stub `g b` does nothing |
| AC6 | `crates/api/tests/browse.rs` :: `ac6_feed` (table) | parameters, the header, `cursor`, the 5-request cap, kinds, mix fields | stub stops at the first page |
| AC7 | `crates/api/tests/browse.rs` :: `ac7_view_all` + `crates/core/src/ui/browse/tests.rs` :: `ac7_view_all_tab` | no `limit`/`offset`; asked once on first show; replaced, cursor kept; failure keeps the preview | stub never asks |
| AC8 | `crates/api/src/library.rs` :: `ac8_browse_paths` (table) | accepted and refused paths; no request sent | stub accepts every path |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_navigation` (table) | push, tabs, wrap, opening rows, `Backspace`, fresh `g b` | stub `Enter` on a link does nothing |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_rows_and_actions` (table) + `crates/core/src/ui/select.rs` tests :: `ac10_links_not_selected` | queued tracks skip other kinds; a link does nothing | stub queues every row |
| AC11 | `crates/core/src/ui/filter` tests :: `ac11_browse_filter` (table) | a mixed section's fields, a link's title, a filter per tab, more pages | stub matches no link |
| AC12 | `crates/api/tests/browse.rs` :: `ac12_failures` (table) + `crates/app/src/ui.rs` :: `ac12_browse_failed_80x24` | wording per status; *Home* alone fails; the empty page | stub fails with a raw status |
| AC13 | `crates/app/src/ui.rs` :: `ac13_browse_150x42`, `ac13_browse_80x24`, `ac13_browse_narrow_50x20`, `ac13_browse_mixed_80x24`, `ac13_browse_one_section_80x24` | snapshots | no snapshot |
| AC14 | `crates/core/src/ui/keymap/tests.rs` (updated rows) + `crates/core/src/ui/help/tests.rs` (new row) | the command, its default key, help text; no longer skipped | stub leaves it skipped |
| AC15 | At acceptance: read the pages. `cargo xtask wiki` builds them | no test: prose | n/a |
| AC16 | `crates/app/src/config.rs` :: `ac16_hide_browse_pages` (table) + `crates/app/src/main.rs` :: `ac16_tui_state_hide_browse_pages` + `crates/core/src/ui/browse/tests.rs` :: `ac16_hidden_links` (table) | default, file, environment, refusal; reaches the state; hidden links, a dropped section, `[]`, exact match | stub shows every link |

Manual checks for the PR's hands-on section (live Tidal, tmux 150 × 42): `g b` and each root tab; a genre page with its tabs, the strip scrolling past the edge; an album, playlist, artist and mix opened from a section; scrolling a long section (*New Albums*) past its first page, which also checks `limit=50` on `pages/data` ("Not verified"); *Home*, its 17 tabs, a mixed section (*Recently played* holds the account's own history, so the screenshot uses another section); a `HISTORY_*` mix.

## Crate placement

- `tidal-player-core`: the types, `Rows::Links`, `Rows::Mixed`, `Row::Link`, the page and key logic, hiding links (`State::hide_browse_pages`, applied when a page arrives)
- `tidal-player-api`: the requests, DTOs, row rules and path check, in a new `crates/api/src/browse.rs` so `library.rs` does not grow further; the v2 base URL and header stay there
- `tidal-player`: rendering, the tab strip and reading `hide_browse_pages`

## Facts vs. assumptions

**Facts** (live probes, 2026-11-05, country `NG`, `scripts/tidal-browse-probe.sh` and `tidal-browse-probe2.sh`; 26 v1 pages and the v2 feed walked):

- All of `pages/home`, `explore`, `for_you`, `genre_page`, `moods_page`, `hires`, `explore_new_music`, `explore_top_music`, `genre_*`, `mood_*`, `m_*` and `single-module-page/…` answer `200`; every row holds one module
- Module types seen: `PLAYLIST_LIST` (42), `ALBUM_LIST` (20), `TRACK_LIST` (10), `VIDEO_LIST` (6), `PAGE_LINKS` (6), `MULTIPLE_TOP_PROMOTIONS` (4), `MIX_LIST` (4), `ARTIST_LIST` (4), `PAGE_LINKS_CLOUD` (3), `FEATURED_PROMOTIONS` (1)
- Up to 12 listable modules on a v1 page; 11 titles on Hip-Hop run to 169 characters joined. Longest title 117 characters. 11 modules are untitled
- `pagedList.dataApiPath` pages with `limit` and `offset` (`limit=5&offset=5` gave the expected rows for albums, playlists, tracks, artists, links). It is null on `PAGE_LINKS` of `explore`, `genre_page`, `moods_page`, and may hold a query (`pages/data/<uuid>?albumId=10581228`). A `MIX_LIST` answer reports `offset: 0` (0011)
- Link items are `{title, apiPath, icon, imageId}`. One has an empty title (`pages/social_justice`)
- `ALBUM_LIST` items have no `type` key; `PLAYLIST_LIST` items no `publicPlaylist` or `creator` key (they have `creators`), `type` `EDITORIAL` or `ARTIST`; the current DTOs read them (`Option` or `serde(default)`)
- 32 playlists have `numberOfTracks` 0, all video playlists; `pages/videos` holds nothing else after the rules, `pages/tidal_magazine` only `MULTIPLE_TOP_PROMOTIONS`
- `mixType` values in v1: `ARTIST_MIX`, `DAILY_MIX`, `DISCOVERY_MIX`, `NEW_RELEASE_MIX`, `VIDEO_DAILY_MIX`
- Unknown page: `404`, `{"status":404,"subStatus":2001,"userMessage":"Not found"}`
- v2 `home/feed/static` answers `400` (`subStatus` 1002) without `x-tidal-client-version`, and `200` with any value tried, `abc` and `0` included: only its presence counts, so the player sends its own version. It also needs both `locale` and `deviceType` + `platform`
- The feed is two pages (`page.cursor`, then none), 17 modules of `{type, moduleId, title, viewAll, items}`, in `SHORTCUT_LIST`, `HORIZONTAL_LIST`, `HORIZONTAL_LIST_WITH_CONTEXT`, `COMPACT_GRID_CARD`; items are `{type, data}`, with `TRACK`, `ALBUM`, `PLAYLIST`, `ARTIST`, `MIX`. Sections mix kinds (*Recently played*: `MIX`, `PLAYLIST`, `TRACK`, `ALBUM`, `DEEP_LINK` in its `view-all`)
- `view-all` answers `{title, subtitle, description, filters, itemLayout, items}`, no total or cursor, 10 to 200 rows. `limit` and `offset` are honoured by some modules (`RECOMMENDED_USERS_PLAYLISTS`), ignored by others (`DAILY_MIXES` gave 10 with `limit=5`), and `SUGGESTED_RADIOS_MIXES` gave no rows at `offset=5`. In the 5 modules compared the feed's preview is the start of the `view-all`
- v2 mixes: `type` is the mix type (`ARTIST_MIX`, `TRACK_MIX`, `DAILY_MIX`, `DISCOVERY_MIX`, `NEW_RELEASE_MIX`, `HISTORY_MONTHLY_MIX`, `HISTORY_YEARLY_MIX`, `HISTORY_ALLTIME_MIX`), 30-digit IDs, title in `titleTextInfo.text`, subtitle in `subtitleTextInfo.text`
- v2 unknown module: `400`, `{"error":"BAD_REQUEST","httpStatus":400,"subStatus":1002}`, not a `404`
- In the code: `Window::next_offset` advances by the page size asked, `Window::whole` marks lists never asked for more, `Window::hidden` counts rows left out, panes hold any number of tabs, and `g b` is free among the defaults

**Not verified** (checked at acceptance, or the spec is corrected):

- `limit=50` on `pages/data/…`: only `limit=5` was sent there, though `single-module-page` answers with `limit: 50` and 50 items
- `HISTORY_*` and `TRACK_MIX` mixes opening through 0011's `pages/mix` and `mixes/{id}/items`
- Video mixes in v2: none appeared, so `VIDEO` in `type` is carried over from v1's `mixType`
- Whether the content of `pages/artist_experiences`, `pages/credits`, `pages/clean_content` and `pages/genre_kids` fits (not walked); the rules skip what does not
- Other countries: every probe ran on one account

## Decisions

1. **Sections are the tabs of one pane** (user, 2026-11-05), over a list of sections or two columns: one key shows a section's rows, and it reuses the artist page's tabs with their paging, filter and selection
2. **v1 and v2 both** (user, 2026-11-05): v2 for the personal home, v1 for everything editorial
3. **The header carries our own version** (of `tidal-player-api`, which sends it): Tidal only checks that it is there, so nothing is imitated
4. **The root's *Discover* tab** (user, 2026-11-05): *Home*, *Staff picks* and *For you* added above explore's untitled links, with *Staff picks* as the name of `pages/home`
5. **Pages known to be empty are hidden by a setting** (user, 2026-11-05): `hide_browse_pages`, defaulting to `videos` and `tidal_magazine`, rather than names fixed in the code; any other empty page says `Nothing to show here`
6. **A section of one kind uses that kind's `Rows`** (user's review, 2026-11-05): a track is already a row, so wrapping every row in a browse-only enum would have duplicated `Row` and `ListItems` and given every match on `Rows::Tracks` a second case. Only a section that holds several kinds needs `Rows::Mixed`

## Out of scope

- The v2 feed's other vibes (*Staff Picks*, *Uploads* in its header)
- `CATEGORY_PAGES` promotions and `EXTURL` links out of the player
- Videos and video mixes
- `HORIZONTAL_LIST_WITH_CONTEXT`'s context row (*Because you listened to* shows without the album it is about)
- Following a Tidal page from a shell, or `playback` commands for browse
- Tidal's *Shortcuts* as anything other than a section
- Caching pages between runs
