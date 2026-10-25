# 0019 — Confirming removals

- **Status**: implemented (2026-10-25; amended the same day: `d d` removes the row from any list of yours, AC10; the manual checks under "Test plan" are run on the user's machine)
- **Owner**: tech-lead (primary session)
- **Issue**: [#61](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/61)
- **Depends on**: 0004 (implemented: the queue, `RemoveFromQueue`), 0006 (implemented: the actions popup, favorites and playlist writes, the `y/n` question), 0008 (implemented: the keymap, `[[actions]]`, `app.toml`), 0012 (implemented: the queue's filter)
- **Followed by**: multi-select ([#71](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/71)), which reuses this spec's question for several rows
- **User docs**: [`docs/tui.md`](../tui.md) ("Favorites and playlists", "The queue", the key table) and [`docs/config.md`](../config.md) (`confirm_removals`, `RemoveSelectedItem` and `RemoveFromQueue`, the `[[actions]]` table), and their zh-TW copies (AC8)

## Context

Three actions remove something with a single key and no way back but doing it again by hand:

- ***Remove from favorites*** (a track, album, artist or playlist you follow): adding it again puts it at the top of *Date added*, not where it was
- ***Remove from this playlist***: adding it again puts it at the end, not at its position
- ***Remove from queue*** (`d` on the queue page, or the popup entry): the entry is gone, the playing one skips to the next

A stray `d` or a wrong row in the actions popup does it at once. Only *Delete playlist* (and adding a duplicate to a playlist) asks today, with the `y/n` question of 0006 (`Popup::Confirm`). spotify-player asks before such actions too (issue #61).

This spec makes the three removals ask with the same question, lets the user turn that off, and gives every list of yours one remove key, `d d` (the queue's was `d`): it removes the row under the cursor from the list it is in (the queue, a playlist of yours, your favorites), so `d` alone is free for removing a selection later (#71). tidalt had no confirmations; nothing is carried over.

## Behaviour

### What asks

| Action | From | Question |
|---|---|---|
| *Remove from favorites* | the actions popup; an `[[actions]]` binding of `DeleteFromLiked`; `RemoveSelectedItem` (`d d`) on your favorites (below) | `Remove <title> from favorites? (y/n)` |
| *Remove from this playlist* | the actions popup on an own playlist's page; `DeleteFromPlaylist`; `d d` there | `Remove <title> from <playlist>? (y/n)` |
| *Remove from queue* | `d d` and the `RemoveFromQueue` command on the queue page; the actions popup; the `RemoveFromQueue` action | `Remove <title> from the queue? (y/n)` |
| *Delete playlist* (asks as before) | the actions popup; `DeletePlaylist`; `d d` on an own playlist in the library | `Delete <playlist>? (y/n)` |

`<title>` is the row's title as the list shows it (a track's title, an album's, an artist's name, a playlist's title); `<playlist>` is the playlist page's title.

The question is 0006's popup: drawn centred over the page, wrapped to the page's width. `y` does the action exactly as it does today (the same request or command, the same message afterwards: `Removed from favorites`, `Removed from <playlist>`, …); `n` or `Esc` (`ClosePopup`) closes it and nothing is sent. While it is open no other key acts (0006). The row is the one the question was asked about, even if the list moved underneath meanwhile (the queue advanced, a page loaded): the question holds what to remove (an entry ID, a favorite's ID, a playlist position and ETag), not a cursor.

Adding a track already in a playlist keeps its question (`Already in <playlist>: add again? (y/n)`); it is not a removal.

### Several items

The question names what it removes as a list of items, each with its title. This spec only ever asks about one item. Multi-select (#71) will ask about several, and the text for that is fixed here so both read alike:

| Items | Question |
|---|---|
| 1 | `Remove <title> from favorites? (y/n)` (as above) |
| N > 1, favorites | `Remove N <kind>s from favorites? (y/n)`: `tracks`, `albums`, `artists`, `playlists` |
| N > 1, a playlist | `Remove N tracks from <playlist>? (y/n)` |
| N > 1, the queue | `Remove N tracks from the queue? (y/n)` |

What `y` sends for several items (one request each, or one for several playlist positions) is #71's to specify. A playlist's positions shift after each removal, so it is not "one `RemoveFromPlaylist` per item".

### `d d`: remove the row from its list

A new command, `RemoveSelectedItem`* (ours; help text `remove from the list`), removes the row under the cursor from the list it is in, through the same question. Its default binding is `d d`. What it does depends on the window (on the filtered row when a filter is set, 0012):

| Window | `d d` |
|---|---|
| The queue | *Remove from queue* |
| An own playlist's tracks | *Remove from this playlist* |
| Favorite tracks (`g y`) | *Remove from favorites* (the track) |
| The library's Albums, Artists | *Remove from favorites* (the album, the artist) |
| The library's Playlists: one you follow | *Remove from favorites* (the playlist) |
| The library's Playlists: one of yours | *Delete playlist* (`Delete <playlist>? (y/n)`, asked whatever `confirm_removals` says) |
| Any other list: an album's or a followed playlist's tracks, an artist page, search results, mixes, a mix, a radio | nothing (the list is not yours) |

On an empty window, with no row under the cursor, nothing happens. The keys help lists `d d` only where it acts (0008's rule), so 0013's hint for `d` shows only there.

`RemoveFromQueue` stays a command (0004/0006), acting on the queue only, and has **no default key** now: `d d` does the same on the queue. No default binding starts with `d` but `d d`, so 0008's prefix rule holds. `d` alone does nothing by default; #71 will make it remove the selected rows. A user's `keymap.toml` can bind either command to `d`, as with any command.

### Keymaps written for `d`

A `keymap.toml` copied from an earlier `examples/keymap.toml` binds `RemoveFromQueue` to `d`. Under 0008's prefix rule as it stood, `d` next to the new default `d d` was an error and the player would not start. 0008 decision 4 is amended: when a sequence the **file** binds and a **default** binding conflict (either starts the other), the file wins and the conflicting defaults are dropped. The TUI's message row says so once at start, with 0008's keymap notice (joined by ` · `):

- one: `keymap.toml: "d" replaces the default "d d" (RemoveSelectedItem)`
- several: `keymap.toml: "g" replaces the defaults "g g" (SelectFirstOrScrollToTop), "g l" (LibraryPage), …` (in the defaults' order)

A conflict between two sequences the file binds stays an error, as 0008 says. The file's own entries are applied first, so a file that rebinds a default sequence (`d d`) to something else and binds `d` too is judged on its own bindings.

### Turning it off

`app.toml` gains, read by the TUI only (the daemon accepts and ignores it, as `key_hints`):

| Key | Accepted | Default | Overridden by |
|---|---|---|---|
| `confirm_removals` | `true`, `false` | `true` | `TIDAL_PLAYER_CONFIRM_REMOVALS` (`on`, `off`) |

With `confirm_removals = false` the three removals act at once, as before this spec. *Delete playlist* and the duplicate question still ask: deleting a playlist cannot be undone at all. `confirm_removals` does not change the keymap: `RemoveSelectedItem` stays on `d d` either way.

## Acceptance criteria

- **AC1** — *Remove from favorites*, from the actions popup or a `DeleteFromLiked` binding, on a track, an album, an artist and a followed playlist, opens the question `Remove <title> from favorites? (y/n)` and sends nothing; `y` then sends `RemoveFavorite(kind, id)` and remembers the write as before (the same message on success); `n` and `Esc` close it and send nothing (table)
- **AC2** — *Remove from this playlist*, from the popup or a `DeleteFromPlaylist` binding, opens `Remove <title> from <playlist>? (y/n)`; `y` sends `RemoveFromPlaylist` with the row's position and the ETag the page had **when the question was asked** (a page reload in between does not change them); `n`/`Esc` send nothing
- **AC3** — `RemoveSelectedItem` and `RemoveFromQueue` on the queue page, the popup's *Remove from queue* and a `RemoveFromQueue` action binding open `Remove <title> from the queue? (y/n)`; `y` sends `RemoveFromQueue` with the entry ID of the row asked about, even after the cursor moved or the queue changed (another entry finished playing, a snapshot arrived); `n`/`Esc` send nothing. On an empty queue nothing opens. With the queue filtered (0012) the question is about the filtered row under the cursor
- **AC4** — With `confirm_removals` off, AC1–AC3's and AC10's removals send at once with no question (today's behaviour); *Delete playlist* and the duplicate question still ask (table over the setting × the five questions)
- **AC5** — The question's text for one and several items: `question(&Removal) -> String` gives the table under "Several items" for favorites of each kind, a playlist and the queue, with N = 1 and N = 3 (table; the N = 3 rows are reachable only through #71 later)
- **AC6** — The default keymap binds `RemoveSelectedItem` to `d d`, nothing to `d`, and nothing to `RemoveFromQueue` (still a valid command name in `keymap.toml`); `d` alone on the queue page sends nothing and leaves `d` pending; `d d` opens AC3's question; `examples/keymap.toml` matches the defaults (`RemoveSelectedItem` on `d d`, no `RemoveFromQueue` entry, as it lists bound defaults only) and its header states AC9's prefix rule (two sequences of the file may not start one another; one that starts a default replaces it); the keys help lists `d d` where it acts (AC10's table) and not elsewhere, and the key hints follow it; `docs/config.md`'s commands table has `RemoveSelectedItem`*
- **AC7** — `confirm_removals` in `app.toml` and `TIDAL_PLAYER_CONFIRM_REMOVALS` set the TUI's setting with 0008's precedence and errors (a non-boolean value or variable: an error naming the key; exit 2); the attached and standalone TUI both copy it into the model; `examples/app.toml` lists it with its default
- **AC8** — Rendering: the question over a page (`insta`, 80 × 24) for a removal from favorites; no panic at any size (0006 AC17's table, new row). `docs/tui.md`, `docs/config.md` and their zh-TW copies describe the questions, `confirm_removals` and `d d`; `README.md` and `CLAUDE.md` "Features" updated
- **AC9** — Building the keymap (0008 AC2's table, new rows): a file binding `RemoveFromQueue` to `d` builds, `d` removes from the queue (with AC3's question) and `d d` is unbound, with the notice `keymap.toml: "d" replaces the default "d d" (RemoveSelectedItem)`; a file binding `g` drops every default `g …` with one notice naming them; a file binding both `s` and `s l a` is still an error; the notice joins the unsupported-names notice with ` · `. `docs/config.md` and its zh-TW copy describe the rule. The binary started with such a file (`-c` a temp dir holding it) runs, not exit 2
- **AC10** — `RemoveSelectedItem` (`d d`) per window, as in the table under "`d d`: remove the row from its list" (table over the windows): the queue → AC3's question; an own playlist's tracks → AC2's question with that row's position and the page's ETag; favorite tracks, the library's Albums and Artists, a followed playlist in the library's Playlists → AC1's question with that kind and ID; an own playlist in the library's Playlists → `Delete <playlist>? (y/n)`, `y` sends `DeletePlaylist`, also with `confirm_removals` off; a followed playlist's tracks, an album's tracks, each artist-page window, each search window, mixes, a mix, a track radio and an artist radio → nothing sent, no popup, state unchanged; an empty window → nothing; with a filter (0012) → the filtered row under the cursor

## Edge cases & errors

- **The item changed while asking**: the question holds IDs and positions, not the cursor. A queue entry already gone (it played to the end and autoplay dropped it, another client removed it): the player ignores `RemoveFromQueue` for an unknown entry, as it does today. A playlist changed elsewhere: the ETag no longer matches and 0006's message says so (`The playlist changed: nothing was changed, try again`)
- **Disconnected** (attached TUI, daemon gone): the question still opens; `y` then does what the action does today while disconnected (a library write is not sent)
- **A long title**: the question wraps to the page's width (0006's popup); a tiny page area draws what fits and never panics
- **A whole-list load in progress**: only `Esc`/`q` act (0006), so no question opens
- **`[[actions]]` binding**: asks exactly as the popup does (the user's choice: a bound key is no reason to skip it)
- **A user keymap binding `d`** (to `RemoveFromQueue` or anything else): the default `d d` is dropped with a notice (AC9); the player starts. With an old `d` → `RemoveFromQueue`, `d` keeps removing from the queue (now asking) and the library has no remove key until the user binds `RemoveSelectedItem`
- **A row whose favorite state is unknown** (0006: the popup cannot tell whether an item is a favorite): only the windows that list favorites act, so every row `d d` removes from favorites is one
- **A non-streamable row** (dimmed, 0006): it can still be removed
- **The library's Playlists before its page loaded which playlists are yours**: ownership comes from the loaded row, as the popup's *Delete playlist* does

## Test plan

Each test is named after its criterion. Red is a failing assertion against stubs with stub bodies (the removals send at once as today, `question` returns an empty string, the config ignores the key, the default keymap keeps `d`), no `todo!()` and no compile errors.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/browse/tests.rs` :: `ac1_confirm_remove_favorite` (table: kind × popup/binding × `y`/`n`/`Esc`) | question text, nothing sent before `y`, the write after `y`, nothing after `n`/`Esc` | the write is sent at once, no popup |
| AC2 | `crates/core/src/ui/browse/tests.rs` :: `ac2_confirm_remove_from_playlist` | question, position and ETag kept across a page reload | the write is sent at once |
| AC3 | `crates/core/src/ui/browse/tests.rs` :: `ac3_confirm_remove_from_queue`; `crates/core/src/ui/filter/tests.rs` :: `ac3_confirm_remove_filtered_queue` | question, the entry asked about after cursor and snapshot changes, empty queue, filtered queue | the command is sent at once |
| AC4 | `crates/core/src/ui/browse/tests.rs` :: `ac4_confirm_removals_off` (table) | sends at once when off; delete and duplicate still ask | stub `State::confirm_removals` is read nowhere and the removals never ask: the "on" rows and the delete row pass, the test fails on the missing questions it expects with the setting on |
| AC5 | `crates/core/src/ui/popup.rs` (a new `#[cfg(test)] mod tests`) :: `ac5_removal_question` (table) | the texts for N = 1 and N = 3 | stub returns `""` |
| AC6 | `crates/core/src/ui/keymap/tests.rs` :: `ac6_remove_selected_item_is_d_d` (renamed from `ac6_remove_from_queue_is_d_d`); `crates/core/src/ui/help/tests.rs` :: `ac6_d_d_listed_where_it_acts`; `crates/core/src/ui/browse/tests.rs` :: `ac6_d_alone_does_nothing`; `crates/app/tests/examples.rs` :: `ac18_example_keymap_is_the_defaults` (existing, fails until the example says `d d`) | default binding, `d` pending, `d d` asks, example | defaults keep `d` |
| AC7 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows); `crates/app/src/main.rs` :: `ac7_tui_state_confirm_removals`; `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` (existing) | value per source, error, model copy, example | stub config ignores the key |
| AC8 | `crates/app/src/ui.rs` :: `ac8_confirm_removal` (snapshot), `ac17_no_panic_any_size` (new row); `crates/app/tests/docs.rs` :: `ac8_confirm_removals_documented`; zh-TW copies, README and `CLAUDE.md` reviewed at acceptance | the drawn question; docs name `confirm_removals` and `d d` | no question drawn; docs lack the terms |
| AC9 | `crates/core/src/ui/keymap/tests.rs` :: `ac9_file_beats_conflicting_defaults` (table), `ac2_build_keymap` (its "`g` bound" row now builds; the `s`/`s l a` row still errors); `crates/app/tests/config.rs` :: `ac9_old_keymap_starts` (`-c` a temp dir with `d` → `RemoveFromQueue`) ; `crates/app/tests/docs.rs` :: `ac8_confirm_removals_documented` (new terms) | the dropped defaults, the notice, the file-only conflict, the binary starts | stub `build` still refuses the conflict (`PrefixConflict`) |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_remove_selected_item` (table: window × own/followed → question or nothing, then `y`); `crates/core/src/ui/filter/tests.rs` :: `ac10_remove_selected_item_filtered` | per window: the question and what `y` sends, nothing on the other windows, the filtered row | stub `RemoveSelectedItem` acts on the queue only (as `RemoveFromQueue`): the queue row passes, every other window fails on a missing question |

Existing tests that press `d` on the queue (`crates/core/src/ui/browse/tests.rs`, `filter/tests.rs`, `search/tests.rs`, `help/tests.rs`) and expect `RemoveFromQueue` at once are updated to this spec in the same change (`d d`, then `y`), never weakened.

Checked by hand at acceptance: in a real terminal, `d d` on the queue asks and `y` removes; `d` then `Esc` does nothing; *Remove from favorites* in the popup asks; `confirm_removals = false` removes at once.

## Crate placement

- `tidal-player-core::ui`: `Confirmed` gains `Remove(Removal)`, `Removal { from: RemoveFrom, items: Vec<Removed> }` with `RemoveFrom::{Favorites(FavoriteKind), Playlist { uuid, title, etag }, Queue}` and `Removed { title, target }` (`target`: the favorite's ID, the playlist position or the `EntryId`), and the pure `question(&Removal) -> String`; `State` gains `confirm_removals: bool` (default `true`); `UiCommand` gains `RemoveSelectedItem` (name, help text, where it acts for the keys help); `keymap::defaults` binds it to `d d` and leaves `RemoveFromQueue` unbound. A removal opens the question through one function that the popup, the bindings, `RemoveFromQueue` and `RemoveSelectedItem` share (`RemoveSelectedItem` picks the row's removal among the actions the popup would list for it, by window), so #71 adds a list of items, not a new path. No I/O, no new dependency
- `tidal-player`: config (`confirm_removals`, the variable), `tui_state` copies it, docs test. No protocol change: `y` sends today's `Command`/`LibraryRequest`
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-25, from code): the question is `Popup::Confirm { question, on_yes: Confirmed }` (`crates/core/src/ui/popup.rs`), answered by `popup_fixed_key` and `confirmed` (`crates/core/src/ui/browse.rs`); `Esc` closes it (`ClosePopup`, `help.rs`); the removals run through `browse::run` (`MenuAction::RemoveFavorite`, `RemoveFromPlaylist`, `RemoveFromQueue`), the `[[actions]]` bindings through `run_action` → `run`, and the queue's key through `dispatch::queue_command`; the default `RemoveFromQueue` binding is `d` and no other default starts with `d` (`keymap::defaults`); 0008 refuses a bound prefix of a longer sequence (`check_prefixes`); `logout` and clearing the queue are not reachable from the TUI (a CLI subcommand; `Command::ClearQueue` has no key).

Assumptions: none about external systems (no new API call).

## Decisions (answered by the user, 2026-10-25)

1. **What asks**: *answered*: removing from favorites, from a playlist and from the queue; deleting a playlist as before. Logout (a CLI command) and clearing the queue (no key) are out
2. **Off switch**: *answered*: yes, `confirm_removals`, default on; *Delete playlist* asks whatever it says
3. **`[[actions]]` bindings**: *answered*: ask as the popup does
4. **Keys**: *answered*: `d d` removes the row under the cursor; `d` is kept for removing the selected rows (#71)
5. **Old keymaps binding `d`** (found at acceptance: the player refused to start with a `keymap.toml` copied from the earlier example): *answered*: the file's binding wins over a conflicting default, with a notice; 0008 decision 4 amended accordingly
6. **`d d` on every list of yours** (asked after the first implementation, 2026-10-25): *answered*: yes, through the same question; on an own playlist in the library it deletes the playlist (*Delete playlist*'s question, always asked); on lists that are not yours (album, artist, search, mixes, radio) it does nothing. Folded into this spec and PR
7. **Command**: a new `RemoveSelectedItem`* bound to `d d` rather than widening `RemoveFromQueue`, whose name says queue; `RemoveFromQueue` keeps working in user keymaps (tech-lead's choice, 2026-10-25)

## Out of scope

- Multi-select and what `d` does with a selection (#71); sending several removals at once
- An undo (re-adding what was removed)
- Asking before `tidal-player logout` or a queue-clearing key
- `d d` on lists that are not yours (decision 6); removing a track from favorites from an album page or search results stays the popup's *Remove from favorites* or a `DeleteFromLiked` binding

## Implementation notes (choices made where the spec was silent, 2026-10-25)

- **Types**: `Removed::target` is `RemovedTarget::{Favorite(String), Position(u32), Entry(EntryId)}`; `RemoveFrom::Playlist` holds the playlist's `uuid` and `title` and the page's ETag as the popup row had it (`Option<String>`, sent as `""` when there is none, as before). The types are exported from `tidal_player_core::ui`; `question` stays in `ui::popup`
- **One path**: `browse::ask_removal(state, Removal)` opens the question, or removes at once with `confirm_removals` off. `browse::run` (popup `Enter` and `[[actions]]` bindings) and `dispatch::queue_command` (`RemoveFromQueue` on the queue page) call it; `run` takes the row's title from the popup's title (`selected_actions`/`playing_actions`), the queue key from the entry's track title
- **Several items**: `y` on a removal of several favorites or queue entries sends one request or command per item. A removal of several playlist positions sends nothing yet: the positions shift after each removal, and #71 specifies how they go
- **Help open**: while the keys help is open, `d` is collected into the pending sequence like any other first key (as `g` was); `ac8_help_no_other_key_acts` now checks `d d` as it checks `g l`
- **Keys help and hint**: the keys help (`?`) lists `d d  remove from the queue` on the queue page, as the existing snapshots `ac14_help_queue` and `ac9_theme_help_80x24` show; a `d` held for the delay shows 0013's hint box titled `d …` with the entry `d  remove from the queue` (0013 lists the next key only)
- **Wrapping**: the question wraps at the popup's width, so the 80 × 24 snapshot shows `Remove Collide With The Sky from favorites?` and `(y/n)` on two lines
- **A user's old `keymap.toml`**: handled by AC9 (decision 5), added after the first acceptance run
- **Test isolation**: `crates/app/tests/cli.rs`'s `bin_in` did not set the config directory, so its tests read the developer's own `~/.config/tidal-player` and failed there with an old `keymap.toml`; it now points `TIDAL_PLAYER_CONFIG_DIR` at the test's temp directory (0008 "Bugs")
- **AC9 notice**: the replaced-defaults clause comes before the unsupported-names one; each starts with `keymap.toml: ` and they are joined by ` · `. Several sequences of the file that each drop defaults give one clause each, joined by `; ` inside a single `keymap.toml: ` prefix. A default that two file sequences both conflict with is named by the first only. A file sequence that is later unbound by `None` replaces nothing
- **AC9 rule**: a "default" is any binding whose sequence the file does not bind; a file sequence that rebinds a default's own sequence is not a conflict. The Quit check still runs after the drop, so dropping every `Quit` key is refused as before
- **AC10 choice of removal**: `browse::removal_action` picks among the actions the popup would list for the row (`selected_actions`), by window: *Delete playlist* else *Remove from favorites* in the library's Playlists, *Remove from this playlist* in an own playlist's tracks, *Remove from favorites* in favorite tracks, Albums and Artists; `browse::removable(page, kind)` is the window rule and is shared with the keys help (`in_window`), so the help and the hint list `d d` exactly where it acts. The queue takes the dispatcher's path (`queue_command`) for both `RemoveSelectedItem` and `RemoveFromQueue`
- **AC10 and a disconnected player**: `d d` on a favorites row asks and `y` sends nothing, as the popup's action does (0006 AC16 test updated: it expected `d` to do nothing)
- **AC10 help text**: `RemoveSelectedItem`'s help text is `remove from the list` on every window, the queue included, so the hint after `d` reads `d  remove from the list` there too
- **Snapshots**: the keys-help snapshots that list `d d` now say `remove from the list`, and the Library · Albums and 40 × 12 help snapshots gain the `d d` row (the window now lists it)
