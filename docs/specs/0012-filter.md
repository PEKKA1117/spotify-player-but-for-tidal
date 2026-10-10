# 0012 — Filter the focused window

- **Status**: draft
- **Owner**: tech-lead (primary session)
- **Issue**: [#18](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/18)
- **Depends on**: 0004 (implemented: the queue page), 0006 (implemented: pages, windows that load as you scroll, the whole-list load, the role filter), 0007 (implemented: the search page and its `/`), 0008 (implemented: the keymap, the `Search` command, fixed keys of text inputs, the keys help and its filter), 0013 (implemented: key hints)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Filtering a list" and the `/` row changes; [`docs/config.md`](../config.md)'s command table changes `Search`'s text (AC10)

## Context

Favorite tracks, a long playlist, an artist's *All tracks* or the queue can hold hundreds or thousands of rows. Today the only way to a row is scrolling (`j`, `C-f`, `G`), and on a list that loads as you scroll the row may not even be loaded yet. spotify-player binds `/` to **`Search`**, "open a popup for searching in the current page": a one-line input that narrows the focused list to the rows that match as you type.

Here `Search` (`/`) does something else: on a search page it gives the search input the focus again (0007 "Windows", 0008's command table), and on every other page it does nothing. 0007 and 0008 both left the in-page filter out of scope and pointed at this spec (0013 calls it "0012, issue #18").

Two pieces of the app already behave like a filter and set the precedents this spec follows:

- the keys help's filter (0008 "The keys help"): `/` starts typing, typed characters narrow the rows ignoring case, `Backspace` edits, `Enter` keeps the filter and stops typing, `Esc` clears it, nothing matching reads `No keys match "xyz"`
- the role filter of *All tracks* (0006): a window shows only some of its loaded rows (`Window::visible`), the title says so, and `Enter` on a track queues **the rows shown**, not the hidden ones

tidalt had no filter, so there is nothing to carry over.

## Behaviour

### Where it acts

`/` (`Search`) starts a **filter** on the focused **window** of the page on top:

- every list window: the library's *Playlists*, *Albums*, *Artists*; *Favorite tracks*; an album's and a playlist's tracks; the artist page's four windows (*Top tracks*, *Albums*, *Appears on*, *All tracks*); the search page's four result windows; the mixes page; a mix's and a radio's tracks
- the **queue** page (its one list, drawn from the player's snapshot)

It does nothing on a page that is loading or failed, on a search page while its input or its *Top hit* row has the focus (the input types `/`), over a popup, the open prompt or a whole-list load. Each window has its own filter.

### Typing

`/` opens the filter for **typing**: the window's title shows it (below) and every printable key and pasted text goes to it, before the keymap (a fixed key, as in the other text inputs, 0008 decision 6). While typing:

| Key | Does |
|---|---|
| printable characters (`space` included), paste | append to the filter (control characters dropped; at most 100 characters) |
| `backspace` | delete the last character (nothing on an empty filter) |
| `C-u` | clear the filter text (typing goes on) |
| `enter` | stop typing; the filter stays |
| `esc` | stop typing and clear the filter |
| `up`, `down`, `page_up`, `page_down` | move the cursor over the matching rows (by one, by a window's height), so a row can be picked without leaving the input |
| `C-c` | quit (as from the search input) |

Every other key is ignored while typing (`q`, `n`, `g …`, `?`, `tab` type or do nothing; rebinding them in `keymap.toml` does not change that). `/` on a window that already has a filter starts typing again with its text, the cursor at its end.

The rows narrow **as you type**: every edit applies at once (filtering is local; nothing is sent per keystroke).

### A kept filter

After `enter` the window keeps its filter, and every key acts as usual on the rows shown: cursor keys, `enter`, `Z`, `g a`, `r`, `d` on the queue, `f` in *All tracks*. Then:

- `esc` (`ClosePopup`) clears the focused window's filter (today it does nothing on a page)
- `/` edits it again
- `tab`/`backtab`, `[`/`]` move to another window, which shows its own filter or none; coming back shows this one as it was
- the filter belongs to the page: going back and forward through the history keeps it, a page fetched again (a reconnect, a playlist edit) keeps it, a newly opened page starts without one, a new search on a search page clears its four windows' filters (they hold new results)

### What matches

A row **matches** when every word of the filter (split on spaces; an empty or all-space filter is no filter) is contained, ignoring case, in at least one of the row's fields:

| Row | Fields |
|---|---|
| track (and queue entry, credit) | title, version, every artist's name, album title |
| album | title, every artist's name, year |
| playlist | title |
| artist | name |
| mix | title, subtitle |

Case is folded with Unicode lower-casing (`RÓS` matches `rós`); accents are not folded (`ros` does not match `Rós`, see "Out of scope"). So `pierce hell` matches *Hell Above · Pierce The Veil*, and `veil 2012` matches an album of that year. In *All tracks* the role filter and the text filter both apply: a row is shown when it has a checked role **and** matches.

### Rows, cursor and actions

- The window shows the matching rows only, in the list's order. The queue page shows the matching entries with their **queue position** (`▶ 2`, `7`, …), and its `Suggested` row only when a suggested entry matches
- The **cursor** stays on its row when that row still matches after an edit; otherwise it goes to the first matching row. On the queue the cursor is an entry ID as before
- **Acting**: `Z`, `g a`, `r`, the actions popup and `d` act on the selected matching row as they would unfiltered (a playlist removal still removes that row's position in the playlist)
- **`enter` on a track** queues the list **as shown** (decision 2): `LoadQueue { the matching tracks, start: the selected one }`. When the list is not complete, the whole-list load (0006) runs first and then queues the matching tracks of the whole list; on the queue page `enter` plays the selected entry as before
- **No match**: the window says `No rows match "xyz"`; no row is selected, so `enter`, `Z`, `g a`, `r`, `d` do nothing

### Lists that load as you scroll

A filter narrows the **loaded** rows, and more are loaded on its behalf (decision 3): while a window has a filter and its list is not complete, the next page is asked whenever fewer than a window's height of matching rows lie below the cursor, the same rule as scrolling (0006 "Lists load as you scroll") measured over the matching rows. It is checked after each filter edit, cursor move and page arrival, one request at a time, so a filter that matches little loads the list page by page until enough rows match or the list is complete, and `Loading more…` shows under the matches meanwhile. `esc` (clearing the filter) stops it: nothing more is asked than unfiltered scrolling would. Lists that arrive whole (mixes, a mix, a radio, 0011) and the queue never load more.

### Drawing

- **Title**: the window's title gains ` · /<filter>` and the match count: `Favorite tracks (548 · /love · 12 matches)`, `All tracks (548 · Performer · 37 hidden · /live · 1 match)`, `Queue (40 · /veil · 9 matches)`. Before the total is known: `Tracks (/love · 3 matches)`. While typing, a cursor block follows the text (`/love▏`) and the title is bold; with an empty filter while typing: `Favorite tracks (548 · /▏)`, every row shown. A title too long for the window is cut with `…` from its end, as today
- **No match**: the window's first row reads `No rows match "xyz"` (the filter's text cut with `…` to fit), with `Loading more…` under it while the list is still loading
- Nothing else moves: the page layout, the list height (`Resize`) and the other windows are unchanged

### The search page

`/` on a search page now filters the focused result window, like everywhere else (decision 1), so `Search` means what it means in spotify-player. Going back to the search input stays on `g s` (`SearchPage` on a search page already focuses its input, 0007) and on `tab`/`backtab`. The search input keeps typing `/`.

### Keys help and key hints

- The keys help lists `/` in every window section (and `Queue`) as **filter the rows**, dim where it would do nothing (a failed or loading page). While typing, the window's section lists the filter's fixed keys instead, as it does for the search input (0008)
- The `Search` row of 0008's command table becomes: acts in "lists", text "filter the rows"
- Key hints (0013) need no change: no default sequence starts with `/`

## Acceptance criteria

- **AC1** — Matching (`filter::matches(row, filter) -> bool`, table): every row kind with the fields above; case folded (`LOVE` ~ `love`, `RÓS` ~ `rós`); words ANDed in any field (`pierce hell`); an empty and an all-space filter match everything; accents not folded; a word found in no field fails
- **AC2** — Typing keys (table): `/` starts typing on each window kind and on the queue, and does nothing where "Where it acts" says so (loading/failed page, the search input, *Top hit*, a popup, the prompt, a whole-list load); printable characters, paste, `backspace`, `C-u`, the 100-character limit; `enter` keeps, `esc` clears, both stop typing; `up`/`down`/`page_up`/`page_down` move over the matches; with `q`→`NextTrack`, `j`→`Quit` and `?` bound, typing `qj?` types it and emits nothing; `C-c` quits; `/` on a kept filter resumes it with its text
- **AC3** — Kept filter (table): keys act on the shown rows; `esc` clears it (and nothing else); `/` edits it; `tab`/`backtab`/`[`/`]` keep each window's filter; back and forward through the history keep it; a page refetch keeps it; a new page has none; a new search clears the search windows' filters
- **AC4** — Rows and cursor: the shown rows are the matches in list order (with the role filter, both apply); the cursor stays on a still-matching row after an edit, else goes to the first match; no match → no selected row (`enter`, `Z`, `g a`, `r`, `d` emit nothing); on the queue the cursor stays an entry ID and moves over matching entries only
- **AC5** — Acting on filtered rows (table): `enter` on track *i* of the matches sends `LoadQueue { matches, start: i }`; on an incomplete list it runs the whole-list load and then sends the whole list's matches with the selected track as `start`; `Z`, the actions popup and `r` act on the selected match; *Remove from this playlist* sends the selected match's position; on the queue `enter` sends `PlayEntry` and `d` `RemoveFromQueue` of the selected match's entry ID
- **AC6** — Loading for a filter (table): a filter on an incomplete list with fewer than a window height of matches below the cursor sends one `More`; another after each reply until enough match or the list is complete; never two at once; none for whole lists and the queue; after `esc` no `More` beyond the unfiltered rule
- **AC7** — The search page: `/` on a result window filters it and does not move the focus to the input; `g s` and `tab` still reach the input; the input types `/`; `/` on *Top hit* does nothing
- **AC8** — Keys help: `/` is listed as "filter the rows" in each window section and `Queue` (one case with `Search` rebound: the moved key shown); dim on a loading or failed page; while typing, the section lists the filter's fixed keys
- **AC9** — Rendering (`insta`, reviewed by eye, + `contains`): at 80 × 24 favorite tracks with a kept filter, the same while typing, no match, the queue filtered (positions and `Suggested`), *All tracks* with a role filter and a text filter, a search window filtered; at 40 × 12 a filtered window; the title texts above; no panic from 1 × 1 to 200 × 60 with a filter set (0004 AC17's test, new rows)
- **AC10** — Docs: `docs/tui.md` has "Filtering a list" and the new `/` row, `docs/config.md`'s command table has `Search`'s new text, their zh-TW copies match, `README.md` and `README.zh-TW.md` mention the filter; 0007's and 0008's text about `/` is updated to point here

## Edge cases & errors

| Case | Behaviour |
|---|---|
| Filter typed on a list of 40 000 loaded rows | Matching runs once per edit or page arrival over the loaded rows, not once per drawn row; typing stays responsive (checked by hand at acceptance) |
| A filter matching nothing on a long incomplete list | Pages load one at a time until the list is complete (`Loading more…`); a `429` or other failure shows as the last row (0006), and the loading stops there until the cursor moves; `esc` stops it |
| Disconnected | Filtering the loaded rows works; no `More` is sent (0005); loading resumes on reconnect as scrolling does |
| The queue changes while filtered (a track ends, autoplay adds) | The shown entries are recomputed from the new snapshot; the cursor keeps its entry if it still matches, else the first match |
| A row changes while filtered (a playlist edit refetches the page) | The filter stays and applies to the new rows; the cursor follows AC4's rule |
| Paste with a line break | Control characters are dropped (as the prompt and search input do) |
| Filter text longer than the title can show | The title is cut with `…`; the filter still applies in full |
| `/` on an empty window | Typing starts; `No rows match "x"` once something is typed (an empty list's own message while the filter is empty) |
| Terminal under 8 rows | No page is drawn (0004); the filter is kept in the model and keys still edit it while typing |
| Attached TUI | The filter is the client's own: nothing goes over the socket but the usual `More` and commands |

## Test plan

Each test is named after its criterion. Red is a failing assertion against stub functions with stub bodies (`matches` returns `true`, `/` keeps today's behaviour, the title ignores the filter), no `todo!()` and no compile errors.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/filter/tests.rs` :: `ac1_matches` (table: row × filter → bool) | fields, case, words, empty, accents | stub `matches` returns `true` for every row |
| AC2 | `crates/core/src/ui/filter/tests.rs` :: `ac2_typing_keys` (table), `ac2_typing_ignores_keymap`, `ac2_slash_where_it_does_nothing` | typing, editing, enter/esc, moving, rebinding, resume | stub `/` does what it does today (nothing off a search page) |
| AC3 | `crates/core/src/ui/filter/tests.rs` :: `ac3_kept_filter` (table) | esc clears, `/` edits, per window, history, refetch, new page, new search | stub: no filter is ever kept |
| AC4 | `crates/core/src/ui/filter/tests.rs` :: `ac4_rows_and_cursor` (table, a browse window and the queue) | shown rows, role + text filter, cursor rule, no match | stub `matches` returns `true` |
| AC5 | `crates/core/src/ui/filter/tests.rs` :: `ac5_acting_on_matches` (table) | `LoadQueue` of matches (complete and whole-list), `Z`, actions, radio, playlist removal position, queue `PlayEntry`/`RemoveFromQueue` | stub `matches` returns `true`: the whole list is queued |
| AC6 | `crates/core/src/ui/filter/tests.rs` :: `ac6_loads_for_filter` (table) | `More` per rule; none in flight twice; none for whole lists/queue; none after `esc` | stub: no `More` on a filter edit |
| AC7 | `crates/core/src/ui/search/tests.rs` :: `ac8_search_windows` (0007's, `/` row changed to this spec) + `crates/core/src/ui/filter/tests.rs` :: `ac7_search_page` | `/` filters, focus stays; `g s`/`tab` reach the input | stub `/` focuses the input (today's) |
| AC8 | `crates/core/src/ui/help/tests.rs` :: `ac7_help_contents` (0008's, rows changed) + `ac8_filter_in_help` | text, dim, rebinding, typing section | stub help text "back to the search input" |
| AC9 | `crates/app/src/ui.rs` :: `ac9_filter_*` (snapshots), `ac9_filter_titles` (table), `ac17_no_panic_any_size` / `ac23_no_panic_any_size` (new rows) | titles, rows, no-match text, sizes | stub render ignores the filter |
| AC10 | `crates/app/tests/docs.rs` :: `ac10_filter_documented` + zh-TW copies and README reviewed at acceptance | `tui.md`, `config.md`, both zh-TW copies mention `/` and "Filtering a list" | docs not yet changed |

Checked by hand at acceptance: in a real terminal, on favorite tracks with a few thousand rows, typing a filter narrows the list without lag; a filter that matches nothing loads the list to its end and shows `No rows match`; `enter` on a filtered track plays the matches.

## Crate placement

- `tidal-player-core::ui`: a new `filter` module: `Filter { text: String, typing: bool }`, `matches(Row, &str)` (the queue page uses the entry's track row), the typing keys (a fixed-key handler called from `dispatch::fixed_key`), the `Search` and `ClosePopup` page commands' new meaning, and the near-end rule over matches (in `browse`). `Window` gains `filter: Filter` and its `visible()` (rows shown) folds in both filters; the matching indices are cached and recomputed only when the rows or the filter change. The queue page's filter lives on its `Page` (it has no window). `help` texts and the typing section. No I/O, no new dependency
- `tidal-player`: the titles, the no-match row and the queue page's filtered rows in `ui.rs` and `ui/pages.rs`
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-10, from code): `Search` is bound to `/` and only calls `search::focus_input` (`crates/core/src/ui/dispatch.rs` `page_command`; `keymap::defaults`); `ClosePopup` does nothing on a page; the role filter narrows `Window::visible`, which `Window::row`, `selected`, `tracks` and `len` read, and `play_from` queues `window.tracks()` from `window.cursor`, both over visible rows; `near_end` compares the cursor with `Window::len` (visible rows) and only runs after cursor moves and the role filter; the queue page has no `Window` (`Page::new(Queue)` has none, `render_queue` draws the snapshot); the keys help's filter keys and texts (`help::key`); the search input's fixed keys (`search::input_key`).

Verified (2026-10-10, spotify-player's README "Commands"): `Search` is "open a popup for searching in the current page", default `/`; `SearchPage` is "go to the search page", `g s`; its tips say `Search` works "in the shortcut help page and other pages". Sorting is separate commands (`SortTrackByTitle` `s t`, …).

Assumptions: none about external systems (no API change: the filter is local, `More` is 0006's request).

## Decisions (open: for the user)

1. **What `/` does on a search page**: *proposed*: filter the focused result window, as everywhere (spotify-player's meaning of `Search`); `g s` and `tab` go back to the input. Alternative: keep "back to the search input" on search pages and filter only elsewhere
2. **What `enter` on a filtered track queues**: *proposed*: the matching tracks, from the selected one (what you see is what plays; the role filter's precedent). Alternative: the whole list, from the selected track (spotify-player plays the whole context)
3. **Lists that load as you scroll**: *proposed*: the filter covers the loaded rows and loads more pages while too few rows match, page by page, until enough match or the list is complete. Alternative: the filter covers the loaded rows only and loads nothing on its own (scrolling to the end of the matches loads as today)
4. **Matching**: *proposed*: words ANDed, each contained in any field, case folded, accents not folded. Alternative: the whole filter as one substring (the keys help's rule)

## Out of scope

- Sorting a window (spotify-player's `Sort*` commands)
- Accent-insensitive matching (needs Unicode normalization, a new dependency)
- Searching Tidal from the filter (the search page does that), regular expressions, fuzzy matching
- Filtering the popups (the actions popup, the playlist picker) and the playback window
- Remembering filters across runs (pages are not persisted, 0009)
