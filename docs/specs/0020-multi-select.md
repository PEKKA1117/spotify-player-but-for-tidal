# 0020 — Selecting several rows

- **Status**: draft
- **Owner**: tech-lead (primary session)
- **Issue**: [#71](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/71)
- **Depends on**: 0004 (implemented: the queue), 0006 (implemented: lists, the actions popup, favorites and playlist writes), 0008 (implemented: the keymap, key sequences), 0012 (implemented: filters), 0013 (implemented: key hints), 0017 (implemented: themes), 0019 (implemented: the removal question, `d d`)
- **User docs**: [`docs/tui.md`](../tui.md) (a new "Selecting rows" section, the key table) and [`docs/config.md`](../config.md) (the new commands, `selected_row`), and their zh-TW copies (AC13)

## Context

Every action acts on one row: the row under the cursor (`Enter`, `a`, `d d`, the actions popup) or the playing track. Queueing five tracks from an album, or removing ten from a playlist, takes five or ten rounds of cursor, key, question. spotify-player has no multi-select either; tidalt had none. Nothing is carried over.

This spec adds a **selection**: rows marked in a window, which a family of keys starting with `s` acts on. Keys that act on the cursor row today keep doing so, selection or not. 0019 left `d` alone free for this; the user chose `d s` instead (decision 4), so `d` stays unbound.

## Behaviour

### The selection

Each window that lists rows (every list window of every page, the search page's four windows, the queue) has its own **selection**: a set of its rows, empty at first. Selecting needs no mode: the keys below add rows to it or take them out. Moving the focus to another window keeps each window's selection; a key acts on the focused window's.

A row stays selected while it is in the list, whether the cursor is on it or not, and whether a filter (0012) hides it or not (decision 3). The selection is **cleared**:

- by `Esc` (`ClosePopup`) when no popup, prompt or filter input is open: the focused window's selection (and motion mode, below)
- by changing page (any page change: `g …`, `Enter` on an album, back and forward in the history): every selection on the page left. Coming back through the history does not bring it back
- by an action on the selection that succeeds (below): that window's selection. An action that fails (a refused write, a changed playlist) leaves it as it was

A row is held by what it is, not by where the cursor is: a queue entry by its entry ID, a list row by its position in the window's loaded list (positions only grow as pages load, 0006). A queue entry that leaves the queue (it finished and autoplay dropped it, another client removed it) leaves the selection with it. A window whose list is reloaded (a write's refresh, 0006) loses its selection.

### Keys

All the new keys are sequences starting with `s` (decision 1). `s` alone is a prefix and does nothing; 0013's hint lists what follows it.

| Keys | Command | Does |
|---|---|---|
| `s c` | `SelectCurrent`* | select the row under the cursor, or unselect it if it is selected |
| `s j`, `s <N> j` | `SelectDown`* | select `N` rows (default 1) from the cursor down, the cursor row included; the cursor ends on the last |
| `s k`, `s <N> k` | `SelectUp`* | the same, upwards |
| `s m` | `SelectMotion`* | turn motion mode on, or off (below) |
| `s a` | `SelectionActions`* | open the selection actions popup (below) |
| `d s` | `RemoveSelection`* | remove the selected rows from their list, asking first (below) |

`*`: ours (0008's mark). Help texts: `select / unselect the row`, `select rows down`, `select rows up`, `motion mode on / off`, `actions on the selection`, `remove the selection`.

**Counts.** `<N>` is one or more digit keys (`0`–`9`) typed after `s` and before `j` or `k`: `s 5 j`, `s 1 2 k`. A count goes in a sequence in place of a key: a binding of `SelectDown` or `SelectUp` to the sequence `s j` (by default, or as `keymap.toml` binds it) also accepts digits before its last key. Only these two commands take a count (decision 2). A count of 0, or digits followed by any other key, cancels the sequence (0008's mismatch: nothing happens). `s 1 j` is `s c` without the unselect. Selecting a row already selected keeps it selected; `SelectDown`/`SelectUp` never unselect. Rows past the end of the list are not there: `s 9 j` on the third-last row selects three rows. In a list that loads as you scroll, rows that are not loaded yet are not selected; the cursor's move loads the next page as any move does (0006).

**Motion mode.** `s m` starts it at the cursor row, the **anchor**. While it is on, every row from the anchor to the cursor (either way, the cursor row included) is shown as selected, however the cursor moves (`j`, `k`, `G`, `g g`, `C-f`, …). `s m` again ends it and adds those rows to the selection. Rows selected before it started stay selected. While it is on:

- the window's title shows `· motion` (below)
- `s a` and `d s` act on the selection with the motion range in it, and end motion mode
- `s c` does nothing: the cursor is always the range's moving end, so its row is always in the range; the keys help dims `s c` (decision 5). To toggle single rows, end motion mode with `s m` first
- `Esc` ends it and clears the window's selection (as above), range included
- changing page or focus ends it: changing page clears as above; moving the focus to another window adds the range to the selection, as `s m` does
- with a filter set, the range is the visible rows between the anchor and the cursor

**Keys on the cursor row.** `Enter`, `a`, `d d` and the other row keys keep acting on the row under the cursor, as today, whatever is selected (decision 6).

### The selection actions popup

`s a` with a selection opens a popup titled `N selected`, listing the actions that apply to every selected row (decision 7). With nothing selected it does nothing (and the keys help dims it, 0008's rule). The actions, in order, each with a fixed key:

| Action | Key | Listed when | Sends |
|---|---|---|---|
| *Play* | `p` | always | `Open { items, at: None }`: the selected rows replace the queue and the first plays |
| *Play next* | `N` | always | `Open { items, at: Some(Next) }` |
| *Add to queue* | `a` | always | `Open { items, at: Some(End) }` |
| *Add to playlist…* | | the rows are tracks (or queue entries) | opens 0006's playlist picker for those tracks, in order |
| *Add to favorites* | | some selected row is not a favorite as far as the list knows (0006) | one `AddFavorite` per such row |
| *Remove from favorites* | | in the windows where `d d` removes from favorites (0019's table) | as `d s` there |
| *Remove from this playlist* | | an own playlist's tracks | as `d s` there |
| *Remove from queue* | | the queue | as `d s` there |

`items` are the selected rows in the list's order (the queue's order for the queue; the loaded list's for others, hidden rows included), each as the `Item` its row's `Enter` or `a` sends today (a track, an album or a playlist). An artist is not an `Item`, so *Play*, *Play next* and *Add to queue* are not listed in an artist window. Navigation actions (*Open*, *Go to album*, *Go to artist*, *Go to radio*) are never listed: they act on one row.

The popup takes 0006's popup keys (`j`/`k`, `Enter` on the cursor row, `Esc`) and the fixed keys of the table: typed straight after `s a` (`s a p`), a fixed key runs its action with no visible pause, and the popup closes. `p`, `N` and `a` inside the popup are the popup's own keys (like the question's `y`/`n`), not keymap bindings, so they do not conflict with `s a` under 0008's prefix rule (decision 8).

An action that succeeds clears the window's selection; *Add to playlist…* clears it once the tracks are added (the picker cancelled with `Esc` keeps it).

### Removing the selection: `d s`

`d s` removes the selected rows from the list they are in, as 0019's `d d` does for one row, through 0019's question, on the same windows (0019's table: the queue, an own playlist's tracks, favorite tracks, the library's Albums, Artists and Playlists). Elsewhere, and with nothing selected, it does nothing. In the library's Playlists, own playlists among the selection are deleted and followed ones removed from favorites (decision 9).

The question names its targets. 0019's text for several items is kept as the first line, and the popup **lists every target** below it, one title per line in the list's order, hidden rows included (decision 3):

```
Remove 3 tracks from Road trip? (y/n)
  Collide With The Sky
  Bloom
  Holocene
```

A list longer than the popup scrolls with `j`/`k` (the question's `y`/`n`/`Esc` act as before). With own playlists in the library's selection, the question is `Delete 2 playlists and remove 1 from favorites? (y/n)`, the deleted ones listed first under `Delete:`, the others under `Remove from favorites:`; it is asked whatever `confirm_removals` says, as *Delete playlist* is (0019). Otherwise `confirm_removals = false` removes at once, as for one row. With one target the question is 0019's one-item text and the list has one line.

What `y` sends:

| From | Sends |
|---|---|
| the queue | one `RemoveFromQueue(entry)` per selected entry |
| favorites | one `RemoveFavorite(kind, id)` per row |
| an own playlist | one `RemoveFromPlaylist { uuid, indices, etag }` with every selected position: the API removes them in one write (assumption A1); the page then reloads as after one removal |
| the library's Playlists | one `DeletePlaylist` per own playlist, one `RemoveFavorite(Playlist, …)` per followed one |

The message afterwards names the count: `Removed 3 tracks from Road trip`, `Removed 3 tracks from the queue`, `Removed 2 albums from favorites`. If some writes of several fail, the message says how many did not (`Removed 2 albums from favorites; 1 failed`) and the selection keeps the failed rows.

### How it looks

- A selected row is drawn with a `+ ` marker before its first column (a row that is not selected gets two spaces, so the columns do not move once anything is selected; a window with an empty selection draws no marker column at all) and in the new theme component **`selected_row`** (ours; default `Bold`), laid over the row's style before the cursor's `selection` (0017's order: the cursor row stays visibly the cursor)
- The window's title gains `· N selected` (` · N selected, M hidden` when a filter hides some), and ` · motion` while motion mode is on, after 0012's match count
- The queue page's rows, the search windows and every list window draw it the same way; popups have no selection

### Keymap

The defaults gain the six bindings above. No default binding was `s` or started with `s` (0008's table), so 0008's prefix rule holds; a `keymap.toml` that binds `s` (or `d`) drops the conflicting defaults with 0019's notice. `examples/keymap.toml` lists them. The keys help lists them under `Lists` where they act (a window with rows; `s a` and `d s` dim with nothing selected), and 0013's hint after `s` lists them.

## Acceptance criteria

- **AC1** — `SelectCurrent` (table over list windows, the queue, a search window): an unselected cursor row becomes selected, a selected one unselected; the cursor does not move; on an empty window nothing changes; with a filter, the row under the cursor is the filtered one
- **AC2** — `SelectDown`/`SelectUp` with counts (table): `s j` selects the cursor row only; `s 3 j` the cursor row and the 2 below, cursor on the last; `s 3 k` upwards; `s 1 2 j` reads 12; past the end (or start) it stops at the last (first) loaded row; already selected rows stay selected; `s 0 j`, `s 3 x`, `s 3 esc` select nothing and leave nothing pending; a lazy list's next page is requested when the cursor reaches the end, as for `j`
- **AC3** — Counts in the keymap: `build` accepts digits before the last key of a `SelectDown`/`SelectUp` binding only: with `SelectDown` rebound to `x y`, `x 4 y` selects 4 rows and `s 4 j` nothing; digits in a sequence of any other command are not a count (`g 4 g` mismatches); a binding whose sequence is a digit (`5` → `NextTrack`) still fires outside a pending count sequence
- **AC4** — Motion mode (table): `s m` sets the anchor; after `j j` three rows read as selected; `s m` adds them and ends the mode; earlier selected rows stay; moving back past the anchor selects the rows the other way; `G` and `g g` extend it; `Esc` ends it and clears the selection; changing focus adds the range; changing page clears; `s a` and `d s` act on selection plus range and end the mode; `s c` changes nothing and the keys help dims it
- **AC5** — Clearing (table): `Esc` with no popup or input open clears the focused window's selection only; `Esc` that closes a popup or a filter input clears nothing; a page change clears every window of the page left, and going back does not restore it; a successful action clears; a failed one keeps the selection; a reloaded window loses it; a queue entry that leaves the queue leaves the selection; focus changes keep each window's selection
- **AC6** — Filter: selected rows hidden by a filter stay selected and are acted on; clearing the filter shows them still selected; selecting through a filter selects the visible rows only
- **AC7** — Selection actions popup: `s a` with nothing selected opens nothing; with a selection it opens titled `N selected` with the actions of the table listed per window (table over windows: an album's tracks, an own playlist, the queue, favorite tracks, the library's Albums, Artists, Playlists, a search window, a mix); no navigation action is ever listed
- **AC8** — Selection actions send (table): *Play* (`s a p`), *Play next* (`s a N`), *Add to queue* (`s a a`) send `Open` with the selected items in list order, hidden rows included, and the `at` of the table, then clear the selection; `Enter` on a popup row does the same as its key; *Add to playlist…* opens the picker with the tracks in order, and the selection clears once added and stays on `Esc`; *Add to favorites* sends one `AddFavorite` per row not known to be a favorite
- **AC9** — Cursor-row keys ignore the selection: with rows selected, `Enter`, `a`, `d d` and `[[actions]]` bindings act on the row under the cursor exactly as without a selection, and leave the selection as it was
- **AC10** — `d s` per window (0019 AC10's table, with a selection of 3): the queue, an own playlist's tracks, favorite tracks, Albums, Artists, followed playlists → the question with 0019's several-items first line and the three titles listed, in list order, hidden rows included; `y` sends the table's writes (one `RemoveFromPlaylist` with the three positions and the page's ETag when asked); `n`/`Esc` send nothing and keep the selection; other windows and an empty selection do nothing; one target gives 0019's one-item text; `confirm_removals = false` sends at once
- **AC11** — `d s` in the library's Playlists with own and followed playlists: the `Delete N playlists and remove M from favorites? (y/n)` question with both lists, asked also with `confirm_removals = false`; `y` sends one `DeletePlaylist` per own and one `RemoveFavorite` per followed playlist; the messages for all succeeding and for some failing (`…; 1 failed`), the failed rows staying selected
- **AC12** — Rendering (`insta`, 80 × 24): a list with 2 selected rows (marker, `selected_row` style, `· 2 selected`), the cursor on a selected row, motion mode (`· motion`), a filter hiding a selected row (`· 2 selected, 1 hidden`), the queue with a selection, the selection actions popup, the `d s` question with three titles and with more titles than fit (scrolled); a theme setting `selected_row` colours the marked rows; no panic at any size (0006 AC17's table, new rows). The cell assertion (0017 AC10's style) that `selection` lies over `selected_row` on the cursor row
- **AC13** — Keymap and docs: the defaults bind the six commands as in the table, and no other default starts with `s` or `d` but `d d`; `examples/keymap.toml` matches; the keys help lists them where they act and dims `s a`/`d s` with an empty selection; 0013's hint after `s` lists `c`, `j`, `k`, `m`, `a`; `theme.toml`'s `component_style` accepts `selected_row` and `examples/theme.toml` keeps loading with no notice; `docs/tui.md`, `docs/config.md` and their zh-TW copies describe selecting, the keys and `selected_row`; `README.md` and `CLAUDE.md` "Features" updated
- **AC14** — Several playlist positions in one write: `LibraryRequest::RemoveFromPlaylist` carries `indices: Vec<u32>`; the API sends `DELETE playlists/{uuid}/items/{i,j,k}` with the ETag (recorded fixture), and one position sends today's request unchanged; a `412` leaves the selection and gives 0006's "playlist changed" message

## Edge cases & errors

- **A selection of 0 visible rows** (every selected row hidden by a filter): `s a` and `d s` still act on them; the title says `· 3 selected, 3 hidden`
- **The queue changes underneath** (auto-advance, another client): entries that leave drop out of the selection; *Remove from queue* for an entry gone meanwhile is ignored by the player, as today (0019)
- **A long selection** (hundreds of rows): `Open` takes them in one command; removals from favorites and the queue send one request each, as 0019's several-items note says; the question's list scrolls
- **Disconnected** (attached TUI, daemon gone): selecting works (it is UI state); actions do what one-row actions do today while disconnected, and the selection is kept since nothing succeeded
- **A whole-list load in progress** (0006): only `Esc`/`q` act, so no selection key does
- **The playing row in the queue**: can be selected and removed, as `d d` can
- **Tiny terminals**: the marker column is dropped below 10 columns of window width (the style alone shows the selection); nothing panics
- **A key typed in a text input** (search, filter, prompt): `s` is text there, as every key is (0008)
- **Unplayable rows** (dimmed, 0006): selected and removed like others; *Play* sends them and the player skips them as it does today

## Test plan

Each test is named after its criterion. Red is a failing assertion against stubs (the commands exist and do nothing, the popup lists nothing, the render draws no marker), no `todo!()` and no compile errors.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/select/tests.rs` :: `ac1_select_current` (table) | toggle, cursor kept, empty window, filtered row | selection stays empty |
| AC2 | `crates/core/src/ui/select/tests.rs` :: `ac2_select_rows_with_count` (table) | rows and cursor per count and direction, ends, cancels, lazy load request | selection stays empty |
| AC3 | `crates/core/src/ui/keymap/tests.rs` :: `ac3_count_in_sequence` (table) | rebound count binding, other commands, digit bindings | `s 4` mismatches |
| AC4 | `crates/core/src/ui/select/tests.rs` :: `ac4_motion_mode` (table) | range, end, both directions, `G`, `Esc`, focus, page, actions | no range |
| AC5 | `crates/core/src/ui/select/tests.rs` :: `ac5_clearing` (table) | each clearing and keeping case | stub selection never set, so the "kept" rows fail |
| AC6 | `crates/core/src/ui/filter/tests.rs` :: `ac6_selection_through_filter` | hidden rows kept and acted on | selection empty |
| AC7 | `crates/core/src/ui/select/tests.rs` :: `ac7_selection_actions_listed` (table over windows) | popup title and actions | popup not opened |
| AC8 | `crates/core/src/ui/select/tests.rs` :: `ac8_selection_actions_send` (table) | the `Open` commands, picker, favorites, clearing | nothing sent |
| AC9 | `crates/core/src/ui/select/tests.rs` :: `ac9_cursor_keys_ignore_selection` (table) | `Enter`, `a`, `d d`, a binding act on the cursor row | stub selection empty, so the "selection kept" assertion fails |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_remove_selection` (table over windows) | question text and list, what `y` sends, `n`/`Esc`, off switch | no question |
| AC11 | `crates/core/src/ui/browse/tests.rs` :: `ac11_remove_selected_playlists` | mixed question, always asked, writes, messages | no question |
| AC12 | `crates/app/src/ui.rs` :: `ac12_selection_*` (snapshots), `ac10_theme_cues` (new cell rows), `ac17_no_panic_any_size` (new rows) | drawn marker, title, popup, question | no marker or title text |
| AC13 | `crates/core/src/ui/keymap/tests.rs` :: `ac13_select_defaults`; `crates/core/src/ui/help/tests.rs` :: `ac13_select_keys_listed`; `crates/core/src/ui/theme/tests.rs` :: `ac13_selected_row_component`; `crates/app/tests/examples.rs` (existing example tests); `crates/app/tests/docs.rs` :: `ac13_selection_documented`; zh-TW copies, README and `CLAUDE.md` reviewed at acceptance | defaults, help, hints, theme name, examples, docs | defaults lack the bindings |
| AC14 | `crates/api/src/library.rs` tests :: `ac14_remove_several_positions` (recorded fixture); `crates/core/src/library.rs` :: protocol round trip row | the request path and headers; one position unchanged; 412 | stub sends the first index only |

Checked by hand at acceptance, in a real terminal against the live API: `s 3 j` then `s a a` queues three tracks; `s m`, `j j`, `s m`, `s a p` plays them; `d s` on three tracks of an own playlist lists them and `y` removes them in one write (A1); `Esc` clears.

## Crate placement

- `tidal-player-core::ui`: a new `select` module: `Selection` (per window, a set of row keys: `EntryId` in the queue, the loaded-list position elsewhere; the motion anchor), the pure transitions (toggle, select N, motion start/end, clear, prune on queue change); `State` holds one per window of the current page and one for the queue; `UiCommand` gains the six commands; `keymap` gains the count slot (`SelectDown`/`SelectUp` bindings accept digits before their last key) and the defaults; `Popup::SelectionActions`; 0019's `Removal` takes several items (it already holds `items: Vec<Removed>`) and its question gains the target list; `Component::SelectedRow`. No I/O, no new dependency
- `tidal-player-core::library`: `LibraryRequest::RemoveFromPlaylist { indices: Vec<u32>, .. }` (was one `index`): a protocol change, so the daemon and clients move together (0005: one binary)
- `tidal-player-api`: the several-positions `DELETE`
- `tidal-player`: rendering (marker column, title, popup, scrolling question), docs test
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-26, from code): `Command::Open { items: Vec<Item>, at: Option<InsertAt> }` queues any items (`None` replaces the queue, `Next`/`End` insert) (`crates/core/src/protocol.rs`); the actions popup's entries are `MenuAction` (`crates/core/src/ui/popup.rs`), where `AddToPlaylist` takes a `TrackSource::Tracks(Vec<Track>)`; 0019's `Removal { items: Vec<Removed> }` and `question` already give the several-items first line; the API removes one playlist position per `DELETE playlists/{uuid}/items/{index}` with `if-none-match` (`crates/api/src/library.rs`); the keymap matches fixed sequences only, refuses a bound prefix of a longer sequence between file entries (`check_prefixes`) and drops a conflicting default with a notice (0019); no default binding is `s` or starts with `s`, and `d` only starts `d d`; the theme's `Selection` component is the cursor row (0017).

Assumptions:

- **A1** — `DELETE playlists/{uuid}/items/{i,j,k}` (positions joined by commas) removes several positions in one write. python-tidal's `UserPlaylist.remove_by_indices` sends this; not checked against the live API. Checked at acceptance by hand; if it is refused, the API sends one `DELETE` per position from the highest down, taking each response's new ETag, and AC14 changes to that (the core's request is the same either way)

## Decisions

Answered by the user (2026-10-25 and 2026-10-26):

1. **Keys**: *answered*: no separate selecting mode; a family under `s`: `s c` toggles the cursor row, `s <N> j`/`s <N> k` select N rows, `s m` toggles motion mode (like Vim's visual mode, ended by `s m` again), `s a` the actions on the selection
2. **Counts**: *answered*: only inside `s` (`s <N> j/k`), the cursor row included, a count of 1 being `s c`; no count with `s c`. Counts for other keys (`5j`) are another issue
3. **Filter**: *answered*: selected rows a filter hides stay selected; the removal question lists every target
4. **Removing**: *answered*: `d d` removes the cursor row as today, `d s` the selection; `d` stays unbound
5. **`s c` in motion mode**: *answered*: does nothing (dimmed in the keys help): the cursor row is always inside the range
6. **Cursor-row keys**: *answered*: `Enter` plays the cursor row with its list as today; `s a p` plays the selection
7. **Actions popup**: *answered*: acts on every selected row; *proposed*: lists only actions that apply to several rows (no navigation)
8. **`s a` and its keys**: *answered*: `s a` opens the popup; `p` *Play* (replaces the queue), `N` *Play next*, `a` *Add to queue* are the popup's own keys, so `s a p` is quick to type and 0008's prefix rule holds
9. **`d s` on own playlists in the library**: *proposed*: deletes them and unfollows the followed ones in one question, always asked. Alternative: leave own playlists out (only `d d` deletes)
10. **Clearing**: *answered*: `Esc`, changing page and a successful action clear the selection; moving the focus between windows keeps each window's
11. **Look**: *proposed*: `+ ` marker, `selected_row` component (default `Bold`), `· N selected` in the title
12. **Select all**: *proposed*: out of scope (a lazy list would have to load every page)
13. **Release**: *answered*: major: a minor bump, `1.1.0` → `1.2.0`

## Out of scope

- Counts on other keys (`5j`, `5 d d`)
- Select all, invert selection, selecting by a filter's matches
- A selection that survives a page change or a reload
- Reordering the queue or a playlist by moving the selection
- Navigation actions on several rows
