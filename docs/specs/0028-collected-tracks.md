# 0028 — Collected Tracks: favorite tracks as a pinned playlist

English | [正體中文](zh-TW/0028-collected-tracks.md)

- **Status**: implemented (2026-11-12; approved 2026-11-12)
- **Owner**: tech-lead (primary session)
- **Issue**: [#103](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/103)
- **Depends on**: 0006 (implemented: the library page, the favorite tracks page, whole-list loads, the actions popup, favorites), 0012 (implemented: `/` and its title), 0019 (implemented: the removal questions, `d d`), 0020 (implemented: the selection, `d s`, `s a`)
- **User docs**: [`docs/tui.md`](../tui.md) (pages, the library, the actions popup, confirming removals, selecting rows), [`docs/config.md`](../config.md) (the commands tables), [`docs/playback.md`](../playback.md) (`confirm_removals`), the comments of `examples/keymap.toml` and `examples/app.toml`, `README.md`, and their zh-TW copies (AC10)

## Context

Tidal's *My Collection* puts Tracks next to Playlists, Albums and Artists. The library page (`g l`, 0006) shows Playlists, Albums and Artists; favorite tracks are a page of their own reached only by `g y`, as spotify-player's Liked Songs. Someone looking at the library sees no tracks and does not know the page exists.

Spotify's app answers this with *Liked Songs* pinned at the top of the playlists. This spec does the same: the favorite tracks page becomes **Collected Tracks**, opened from a pinned first row of the library's Playlists window (or `g y`, as before). And as Tidal calls favorites your **collection**, never *favorites* or *liked*, the TUI does too: a track's favorite actions and messages say *Collected Tracks*, an album's, artist's or playlist's say *collection*.

This spec replaces, in 0006: the favorite tracks page's name, title, empty and failure messages ("Pages", "Rows"); the library page's lists (it now fetches the favorite tracks' total too); and, in 0006/0008/0019/0020, every text the TUI shows for adding to or removing from favorites, the empty messages of the library's Albums and Artists, and the keys help texts of `AddToLiked`, `DeleteFromLiked` and `LikedTrackPage`.

## Behaviour

### The pinned row

The library's Playlists window starts with one row that is not a playlist: **Collected Tracks** and the number of favorite tracks, then the playlists as before.

```
┌Playlists (22)───────────────┐
│  Collected Tracks        362│
│  Running                  42│
│♥ Late night               17│
```

- It is drawn as a playlist row of your own (no `♥`: that marks a playlist you follow), always first, whatever the order of the playlists
- The count is Tidal's total of favorite tracks, fetched with the library page. With no favorite tracks it says `0`; a player that sends no total (an older daemon) leaves the count empty
- The window title's count stays Tidal's number of playlists (`Playlists (22)`); paging, `Loading more…` and the next page's offset count playlists only
- The cursor starts on it, as on any first row

| Key / action | On the pinned row |
|---|---|
| `Enter` | opens the Collected Tracks page (as `g y`) |
| `Z`, `C-z`, *Add to queue* | adds every collected track at the end of the queue |
| *Play next* | adds every collected track after the playing one |
| *Add to playlist…* | adds every collected track to the chosen playlist, as for an album |
| `g a`, `C-space` (actions popup) | *Open*, *Add to queue*, *Play next*, *Add to playlist…*: no favorite, rename, description, public or delete actions |
| `/` | matched by its name, `Collected Tracks`, like a playlist's title; counted in the matches |
| `s c`, `s <N> j`/`k`, `s m` | cannot be selected: `s c` on it does nothing, a count or motion that covers it selects the playlists only |
| `d d`, `d s` | do nothing to it (`d d` on it sends nothing and opens nothing) |
| `r`, *Go to radio* | nothing (a list has no radio) |

*Add to queue*, *Play next*, `Z` and *Add to playlist…* need every track: they fetch the remaining pages first, as 0006's "The whole list is needed" (`Loading 300 of 1 234…`, `Esc` cancels, nothing sent; above 40 000 tracks refused the same way), then send one command with every track in list order.

### The Collected Tracks page

What was the favorite tracks page, renamed; its rows, keys, loading, actions and removals are unchanged.

- Title row: `Collected Tracks · 362 tracks`
- Window name (window title, keys help heading, filter title): `Collected Tracks` (`Collected Tracks (548 · /love · 12 matches)`)
- Empty: `No collected tracks yet`
- Failure: `Could not load Collected Tracks: <reason>`
- `g y` still opens it from anywhere; its help text and key hint read `Collected Tracks`

### Collection, not favorites

Tidal calls what you favorite your collection; the TUI no longer says *favorite* or *favorites* anywhere. A **track** goes to *Collected Tracks*; an **album, artist or playlist** to your *collection*.

| Where | Was | Now: a track | Now: an album, artist or playlist |
|---|---|---|---|
| Actions popup | *Add to favorites*, *Remove from favorites* | *Add to Collected Tracks*, *Remove from Collected Tracks* | *Add to collection*, *Remove from collection* |
| Message row | `Added to favorites`, `Removed from favorites` | `Added to Collected Tracks`, `Removed from Collected Tracks` | `Added to collection`, `Removed from collection` |
| Question (0019), one | `Remove <title> from favorites? (y/n)` | `Remove <title> from Collected Tracks? (y/n)` | `Remove <title> from your collection? (y/n)` |
| Question (0019), several | `Remove 3 tracks from favorites? (y/n)` | `Remove 3 tracks from Collected Tracks? (y/n)` | `Remove 3 albums from your collection? (y/n)` |
| Selection popup (0020) | *Add to favorites*, *Remove from favorites* | *Add to Collected Tracks*, *Remove from Collected Tracks* (every selected row a track) | *Add to collection*, *Remove from collection* (any other selection) |
| Selection list question (0020) | `Remove from favorites:` | `Remove from Collected Tracks:` | `Remove from your collection:` |
| Selection results (0020) | `Added 3 tracks to favorites`, `Removed 3 albums from favorites` | `Added 3 tracks to Collected Tracks`, `Removed 3 tracks from Collected Tracks` | `Added 3 albums to collection`, `Removed 3 albums from collection` |
| Own and followed playlists (0020) | `Delete 2 playlists and remove 1 from favorites? (y/n)` | | `Delete 2 playlists and remove 1 from your collection? (y/n)` |
| Library empty lists (0006) | `No favorite albums yet`, `No favorite artists yet` | | `No albums in your collection yet`, `No artists in your collection yet` |

With several kinds in one selection (a browse section of several kinds), each kind's clause takes its own word: `Added 2 tracks to Collected Tracks, 1 album to collection`.

Keys help texts (0008): `AddToLiked` → `add to collection`, `DeleteFromLiked` → `remove from collection (asks y/n first)`, `LikedTrackPage` → `Collected Tracks`. The commands keep their spotify-player names.

### Keeping the count current

A successful change of a track's favorite (add or remove, one or a selection) fetches the library page again when it is in the history, as an album's or an artist's already does, so the pinned row's count follows; the Collected Tracks page is fetched again as before.

## Acceptance criteria

- **AC1** — Fetching the library: `Page(Library)` also asks `users/{user}/favorites/tracks` with `limit=1`, alongside the three lists, and `PageData::Library` carries its `totalNumberOfItems` as `collected_tracks: Option<u32>` (`Some(0)` for none); a failure of that request fails the page as a failure of the other three does. A `PageData::Library` without the field (an older player) deserializes with `collected_tracks: None`
- **AC2** — The pinned row (table): applying `PageData::Library` gives the Playlists window a first row *Collected Tracks* with the count (`Some(n)`, `Some(0)`, `None` → no count), then the playlists; the window's `total` is the playlists' total; the next `More` asks `Playlists` from offset = page size, and `Loading more…` shows while loaded playlists < total; a reload keeps it first. Other windows and pages have no such row
- **AC3** — `Enter` on the pinned row opens the Collected Tracks page (`Page(FavoriteTracks)`, pushed as `g y` pushes it); `g y` with that page on top does nothing
- **AC4** — `Z`/`C-z` and the popup's *Add to queue*, *Play next* and *Add to playlist…* on the pinned row load the favorite tracks whole (`More { FavoriteTracks, … }` until the total, the `Loading N of M…` message, `Esc` cancels with nothing sent, above 40 000 refused), then send one command: `AddToQueue { tracks: <the tracks>, at: End }`, `AddToQueue { tracks: <the tracks>, at: Next }` (the tracks as loaded, so the player looks none of them up), or `AddToPlaylist`; with no favorite tracks nothing is sent and the message row says `Collected Tracks is empty` with 0006's duplicate check (table over the four)
- **AC5** — The actions popup on the pinned row lists exactly *Open*, *Add to queue*, *Play next*, *Add to playlist…*; `r` and *Go to radio* do nothing on it; `d d` on it sends nothing and opens nothing
- **AC6** — Selecting (0020): `s c` on the pinned row selects nothing; `s 3 j` from it selects the two playlists among the three rows it covers; motion mode over it selects only the playlists; `s a` and `d s` never act on it (table)
- **AC7** — Filtering (0012): `/coll` in the library's Playlists keeps the pinned row (and any playlist whose title has `coll`) and counts it in the matches; `/run` hides it; `Enter` keeps the filter and `Esc` clears it with the row back first
- **AC8** — Texts (table): the Collected Tracks page's title row, window name, empty and failure messages; the keys help texts and key hints of `g y`, `AddToLiked` and `DeleteFromLiked`; for a track and for an album, an artist and a playlist: the actions popup labels, message row texts, questions for one and three items; the selection popup labels for a selection of tracks only and of tracks with albums, the selection list question, the selection result clauses (one kind and several), the own-and-followed playlists question; the library's empty Albums and Artists messages, all as in "Collection, not favorites". No text the TUI draws contains `favorite` (a check over every text these tables produce)
- **AC9** — Keeping the count current: a successful `AddFavorite`/`RemoveFavorite` of a track (one, or a selection's) fetches the library page again when it is anywhere in the history, and the Collected Tracks page as before; with no library page in the history nothing more is fetched
- **AC10** — Rendering (`insta`, 80 × 24, reviewed by eye): the library with the pinned row (count, no `♥`, cursor on it), with `0` and with no count; the Collected Tracks page; the actions popup on the pinned row; 50 × 20 narrow library; no panic from 0 × 0 to 120 × 40 (0006 AC17's table). Docs: `docs/tui.md`, `docs/config.md`, `docs/playback.md` (`confirm_removals`), `examples/keymap.toml`'s and `examples/app.toml`'s comments, `README.md` and their zh-TW copies name Collected Tracks, the pinned row and the collection, and say *favorite* only where they explain that Tidal's favorites are the collection; `CLAUDE.md`'s Library entry updated

## Edge cases & errors

- **No favorite tracks**: the row says `0`; `Enter` opens the page with `No collected tracks yet`; `Z` and the popup's list actions send nothing and say `Collected Tracks is empty`
- **The total request fails** (network, 429, expired session): the library page fails with 0006's message, as when any of its lists fails; `Backspace`, then `g l` tries again
- **An older daemon** (no `collected_tracks`): the row is there with no count; everything else works
- **Disconnected**: `Enter` opens the page, which fails with the disconnected message as any page does; the list actions say so as 0006's whole-list loads do
- **Filtered to nothing but the pinned row**: `Enter` opens the page; the matches say `1 match`
- **A huge collection** (above 40 000): `Z` and the list actions refuse with 0006's message; the page still opens and scrolls
- **Tiny terminals**: the row shortens like a playlist row (0006's `playlist_row` rules)

## Test plan

Each test is named after its criterion. Red is a failing assertion against stubs (the library page's `collected_tracks` always `None` and never shown, no pinned row, the old texts), no `todo!()` and no compile errors.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/api/tests/library.rs` :: `ac1_library_page_counts_collected_tracks` (recorded fixtures: a total, `0`, a failing request); `crates/core/src/protocol/tests.rs` :: `ac1_library_page_without_collected_tracks` | the extra request and its `limit=1`, the total carried, the page failing, the missing field read as `None` | the stub sends no request and carries `None` |
| AC2 | `crates/core/src/ui/browse/tests.rs` :: `ac2_collected_tracks_row` (table: `Some(362)`, `Some(0)`, `None`, reload, paging) | first row and count, window total, next offset, `Loading more…`, other windows without it | no pinned row: the first row is the first playlist |
| AC3 | `crates/core/src/ui/browse/tests.rs` :: `ac3_collected_tracks_row_opens_page` | the request and the pushed page; `g y` on top does nothing | `Enter` on the first row opens a playlist |
| AC4 | `crates/core/src/ui/browse/tests.rs` :: `ac4_collected_tracks_row_whole_list` (table: `Z`, *Add to queue*, *Play next*, *Add to playlist…*; `Esc`; above 40 000) | the `More` requests, the message, the one command sent, the cancel and refusal | nothing is loaded or sent |
| AC5 | `crates/core/src/ui/browse/tests.rs` :: `ac5_collected_tracks_row_actions` | the popup's four actions; `r`, *Go to radio* and `d d` do nothing | the popup holds the first playlist's actions || AC6 | `crates/core/src/ui/select/tests.rs` :: `ac6_collected_tracks_row_not_selectable` (table: `s c`, `s 3 j`, motion, `s a`, `d s`) | the selection never holds it | `s c` selects the first playlist |
| AC7 | `crates/core/src/ui/filter/tests.rs` :: `ac7_filter_collected_tracks_row` | kept by `/coll`, hidden by `/run`, counted, back first after `Esc` | the row is never there |
| AC8 | `crates/core/src/ui/popup.rs` :: `ac8_collection_texts` (table: kind × labels, questions N = 1 and 3, selection labels, the selection list question, the playlists question); `crates/core/src/ui/browse/tests.rs` :: `ac8_collected_tracks_texts` (table: title, window name, empty and failure messages, the library's empty Albums and Artists, message row texts per kind); `crates/core/src/ui/select/tests.rs` :: `ac8_selection_collection_clauses` (one kind, several kinds); `crates/core/src/ui/help/tests.rs` :: `ac8_collection_help_texts` (`g y`, `AddToLiked`, `DeleteFromLiked`, the key hint after `g`) | each text per kind; none contains `favorite` | the stubs return the old *favorites* texts |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_track_favorite_refetches_library` (table: add, remove, selection; library in or out of the history) | the library page fetched again or not | only the favorite tracks page is fetched again |
| AC10 | `crates/app/src/tui/render/tests.rs` :: `ac10_library_collected_tracks` (snapshots: count, `0`, none; the narrow library), `ac10_collected_tracks_page`, `ac10_collected_tracks_row_actions`, `ac17_no_panic_any_size` (existing, new rows); `crates/app/tests/docs.rs` :: `ac10_collected_tracks_documented`; zh-TW copies, README, `examples/keymap.toml` and `CLAUDE.md` reviewed at acceptance | the drawn row and page; the docs name *Collected Tracks* and the pinned row | no row drawn; the docs lack the terms |

Existing tests whose expected texts change (*Favorite tracks*, a track's *favorites* texts, the library snapshots) are updated to this spec in the same change, never weakened.

**Hands-on check** (live Tidal, tmux 150 × 42, as in `CLAUDE.md`): the library shows *Collected Tracks* first with the same count as the Tidal app; `Enter` opens the page titled `Collected Tracks · N tracks`; `g y` opens the same page; `/coll` keeps the row; *Add to Collected Tracks* on a track elsewhere, then `g l`: the count is one higher (undo after); `Z` on the row queues every track. By ear: nothing new to check.

## Crate placement

- `tidal-player-core::library`: `PageData::Library` gains `collected_tracks: Option<u32>` with `#[serde(default)]` (AC1)
- `tidal-player-api::library`: the `PageRequest::Library` arm joins a `favorite_tracks(0, 1)` total with the three lists (AC1)
- `tidal-player-core::ui::page`: the pinned row in the library's Playlists window: a `Row` for it, the index mapping that puts it first while `rows`, `positions`, `total` and `next_offset` stay the playlists'; the renamed texts of `WindowKind::FavoriteTracks` and its page (AC2, AC8)
- `tidal-player-core::ui::browse`: `Enter`, `Z`, the actions popup and the whole-list load from the pinned row (a detached `FavoriteTracks` window and a queue purpose beside `Play` and `AddToPlaylist`); the refetch of the library after a track's favorite changes; the messages (AC3–AC5, AC8, AC9)
- `tidal-player-core::ui::popup`, `ui::select`, `ui::filter`, `ui::help`: the texts, the selection and filter rules for the row (AC6–AC8)
- `tidal-player` (`tui/render`): drawing the row (AC10)
- Internal names (`PageKind::FavoriteTracks`, `ListRef::FavoriteTracks`, `LikedTrackPage`, `FavoriteKind`) stay: only what the user reads changes

## Facts vs. assumptions

Facts (from the code, 2026-11-12):

- The library page fetches `playlists`, `favorite_albums` and `favorite_artists` with `tokio::try_join!` (`crates/api/src/library.rs`), so one failure fails the page
- `favorite_tracks(offset, limit)` already returns Tidal's `totalNumberOfItems` as `ListPage::total`; 0006's probe found 362 for the maintainer's account, matching the Tidal app
- 0006's whole-list load can run on a list no page shows (`WholeListSource::Detached`, used for an album row's *Add to playlist…*), with the purposes `Play` and `AddToPlaylist`
- A track's favorite change already fetches the favorite tracks page again; an album's or artist's fetches the library again (`browse.rs`, the reply handling of `AddFavorite`/`RemoveFavorite`)

Assumptions, checked at acceptance:

- `limit=1` on `users/{user}/favorites/tracks` returns the full `totalNumberOfItems` (a recorded fixture from the live API, made while implementing AC1)

## Decisions (answered by the user, 2026-11-12)

1. **Where**: *answered*: favorite tracks become a pinned playlist, *Collected Tracks*, at the top of the library's Playlists window, rather than a fourth library window (which would have changed the layout and `[layout] library`)
2. **`g y`**: *answered*: still opens the page from anywhere; the row and `g y` lead to the same page
3. **Spec**: *answered*: a new spec, this one, naming what it replaces in 0006, rather than rewriting 0006
4. **Filter**: *answered*: the row is matched by its name like a playlist, not always shown
5. **Wording**: *answered*: *Collected Tracks* everywhere a track's favorite is named, not only the page; and, as Tidal says *collection* rather than *favorites* or *liked*, albums, artists and playlists say *collection*, and so do the keys help texts of `AddToLiked`/`DeleteFromLiked`. The alternative that lost: only the help texts, with *favorites* kept in popups and messages
6. **No favorite tracks**: *answered*: the row stays with `0`, as Spotify's Liked Songs, rather than hidden
7. **The count's source**: Tidal's total from a `limit=1` request joined to the library's, failing with it, rather than a separate request that leaves the row without a count on failure (tech-lead's choice, 2026-11-12)
8. **The row's look**: drawn as a playlist of yours (no `♥`), set apart only by being first, rather than a new theme component (tech-lead's choice, 2026-11-12)

## Out of scope

- Sorting the library or its lists (#52); the pinned row will stay first when it comes
- Videos and the profile from Tidal's *My Collection*
- Your favorite tracks on an artist's page (#63)
- Renaming the commands (`LikedTrackPage`, `AddToLiked`, `DeleteFromLiked`) in `keymap.toml`: they are spotify-player's names

## Implementation notes (choices made where the spec was silent, 2026-11-12)

- **The pinned row**: a stand-in `PlaylistSummary` (uuid `collected-tracks`, which no Tidal playlist has; `own: false`, so the *Add to playlist…* list never offers it; `tracks` holds the count) at index 0 of the library's Playlists `rows` and `positions`, inserted by `Window::pin_collected` on every fill of a library page; `Rows::get` reads it as `Row::Collected(Option<u32>)`. `total`, `next_offset` and `complete()` stay the playlists', so the cursor, the filter and the selection need no index mapping
- **Whole-list loads from the row**: `Purpose::Queue { at }` beside `Play` and `AddToPlaylist`, `MenuAction::QueueCollected(InsertAt)` and `TrackSource::Collected`, all through one `load_detached` helper that the album row's *Add to playlist…* now uses too. Above 40 000 is refused as soon as the first page gives the total; disconnected, it does nothing, as the album path does. `[[actions]]` bindings of `AddToQueue` and `PlayNext` act on the row through `QueueCollected`
- **Selection**: *Add to/Remove from Collected Tracks* when every selected row is a track (`tracks_only`), *collection* otherwise. `AddFavorites` now carries each row's own kind (it sent every row with the first row's kind, wrong for a browse section of several kinds). The list question's heading is per kind (`Remove from Collected Tracks:`, `Remove from your collection:`); own and followed playlists together end with `Deleted 2 playlists and removed 1 from collection`
- **Refetch order**: a track's favorite change fetches the Collected Tracks page again, then the library page
- **An empty library**: the Playlists window always holds the pinned row, so it no longer says `No playlists yet` (0006's empty-list message stays for a library with no row, which cannot happen now)
- **API labels**: the `Malformed` error labels that reach the message row (`malformed … response from Tidal`) say `collected tracks`, `collection albums`, `collection artists` and `collection`
- **AC1's protocol test** cannot be red: serde reads a missing `Option` as `None` whatever the stub does; it guards the field's `#[serde(default)]` from here on
- **`crates/app/tests/daemon.rs`** (`ac15_daemon_reads_app_toml`): leaves out the `limit=1` request when it checks the page size of the library's requests

## Bugs

### `Z` on the pinned row queues nothing when one collected track is gone (found at the hands-on check, 2026-11-12)

- **Expected**: `Z` (and *Add to queue*, *Play next*) on *Collected Tracks* adds every collected track to the queue, as AC4 says
- **Actual**: with 373 collected tracks, one of them no longer on Tidal, nothing is added and the message row says `Track 256746446 was not found`
- **Root cause**: AC4 had the client send `Open { items: [Item::Track(id), …] }`, which the player resolves one ID at a time and fails as a whole on the first missing track. The tracks are already loaded with everything the queue needs, so `AddToQueue { tracks, at }` (0004) carries them as they are, as 0006's `LoadQueue` does for playing a list. The spec was wrong, not the code
- **New criterion**: AC4 now names `AddToQueue { tracks, at }`; its test asserts that command, and a row whose list holds a track Tidal no longer has still sends every track. Tracks that are not streamable are queued and skipped by the player, as on any list (0004 AC7)
