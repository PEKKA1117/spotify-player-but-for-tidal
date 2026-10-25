# 0019 — Confirming removals

- **Status**: approved (2026-10-25)
- **Owner**: tech-lead (primary session)
- **Issue**: [#61](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/61)
- **Depends on**: 0004 (implemented: the queue, `RemoveFromQueue`), 0006 (implemented: the actions popup, favorites and playlist writes, the `y/n` question), 0008 (implemented: the keymap, `[[actions]]`, `app.toml`), 0012 (implemented: the queue's filter)
- **Followed by**: multi-select ([#71](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/71)), which reuses this spec's question for several rows
- **User docs**: [`docs/tui.md`](../tui.md) ("Favorites and playlists", "The queue", the key table) and [`docs/config.md`](../config.md) (`confirm_removals`, the `RemoveFromQueue` default, the `[[actions]]` table), and their zh-TW copies (AC8)

## Context

Three actions remove something with a single key and no way back but doing it again by hand:

- ***Remove from favorites*** (a track, album, artist or playlist you follow): adding it again puts it at the top of *Date added*, not where it was
- ***Remove from this playlist***: adding it again puts it at the end, not at its position
- ***Remove from queue*** (`d` on the queue page, or the popup entry): the entry is gone, the playing one skips to the next

A stray `d` or a wrong row in the actions popup does it at once. Only *Delete playlist* (and adding a duplicate to a playlist) asks today, with the `y/n` question of 0006 (`Popup::Confirm`). spotify-player asks before such actions too (issue #61).

This spec makes the three removals ask with the same question, lets the user turn that off, and moves the queue's remove key from `d` to `d d` so `d` alone is free for removing a selection later (#71). tidalt had no confirmations; nothing is carried over.

## Behaviour

### What asks

| Action | From | Question |
|---|---|---|
| *Remove from favorites* | the actions popup; an `[[actions]]` binding of `DeleteFromLiked` | `Remove <title> from favorites? (y/n)` |
| *Remove from this playlist* | the actions popup on an own playlist's page; `DeleteFromPlaylist` | `Remove <title> from <playlist>? (y/n)` |
| *Remove from queue* | `RemoveFromQueue` (`d d`) on the queue page; the actions popup; the `RemoveFromQueue` action | `Remove <title> from the queue? (y/n)` |
| *Delete playlist* (unchanged) | the actions popup; `DeletePlaylist` | `Delete <playlist>? (y/n)` |

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

### The queue's key: `d d`

`RemoveFromQueue`'s default binding becomes `d d` (it was `d`). No default binding starts with `d`, so 0008's prefix rule holds. With key hints (0013), a `d` held for the delay shows `d …` with `d  remove from the queue` on the queue page. `d` alone does nothing by default; #71 will make it remove the selected rows. A user's `keymap.toml` can bind `RemoveFromQueue` back to `d`, as with any command.

### Turning it off

`app.toml` gains, read by the TUI only (the daemon accepts and ignores it, as `key_hints`):

| Key | Accepted | Default | Overridden by |
|---|---|---|---|
| `confirm_removals` | `true`, `false` | `true` | `TIDAL_PLAYER_CONFIRM_REMOVALS` (`on`, `off`) |

With `confirm_removals = false` the three removals act at once, as before this spec. *Delete playlist* and the duplicate question still ask: deleting a playlist cannot be undone at all. `confirm_removals` does not change the keymap: `RemoveFromQueue` stays on `d d` either way.

## Acceptance criteria

- **AC1** — *Remove from favorites*, from the actions popup or a `DeleteFromLiked` binding, on a track, an album, an artist and a followed playlist, opens the question `Remove <title> from favorites? (y/n)` and sends nothing; `y` then sends `RemoveFavorite(kind, id)` and remembers the write as before (the same message on success); `n` and `Esc` close it and send nothing (table)
- **AC2** — *Remove from this playlist*, from the popup or a `DeleteFromPlaylist` binding, opens `Remove <title> from <playlist>? (y/n)`; `y` sends `RemoveFromPlaylist` with the row's position and the ETag the page had **when the question was asked** (a page reload in between does not change them); `n`/`Esc` send nothing
- **AC3** — `RemoveFromQueue` on the queue page, the popup's *Remove from queue* and a `RemoveFromQueue` action binding open `Remove <title> from the queue? (y/n)`; `y` sends `RemoveFromQueue` with the entry ID of the row asked about, even after the cursor moved or the queue changed (another entry finished playing, a snapshot arrived); `n`/`Esc` send nothing. On an empty queue nothing opens. With the queue filtered (0012) the question is about the filtered row under the cursor
- **AC4** — With `confirm_removals` off, AC1–AC3's actions send at once with no question (today's behaviour); *Delete playlist* and the duplicate question still ask (table over the setting × the five questions)
- **AC5** — The question's text for one and several items: `question(&Removal) -> String` gives the table under "Several items" for favorites of each kind, a playlist and the queue, with N = 1 and N = 3 (table; the N = 3 rows are reachable only through #71 later)
- **AC6** — The default keymap binds `RemoveFromQueue` to `d d` and nothing to `d`; `d` alone on the queue page sends nothing and leaves `d` pending; `d d` opens AC3's question; `examples/keymap.toml` matches the defaults; the keys help and the key hints list `d d`
- **AC7** — `confirm_removals` in `app.toml` and `TIDAL_PLAYER_CONFIRM_REMOVALS` set the TUI's setting with 0008's precedence and errors (a non-boolean value or variable: an error naming the key; exit 2); the attached and standalone TUI both copy it into the model; `examples/app.toml` lists it with its default
- **AC8** — Rendering: the question over a page (`insta`, 80 × 24) for a removal from favorites; no panic at any size (0006 AC17's table, new row). `docs/tui.md`, `docs/config.md` and their zh-TW copies describe the questions, `confirm_removals` and `d d`; `README.md` and `CLAUDE.md` "Features" updated

## Edge cases & errors

- **The item changed while asking**: the question holds IDs and positions, not the cursor. A queue entry already gone (it played to the end and autoplay dropped it, another client removed it): the player ignores `RemoveFromQueue` for an unknown entry, as it does today. A playlist changed elsewhere: the ETag no longer matches and 0006's message says so (`The playlist changed: nothing was changed, try again`)
- **Disconnected** (attached TUI, daemon gone): the question still opens; `y` then does what the action does today while disconnected (a library write is not sent)
- **A long title**: the question wraps to the page's width (0006's popup); a tiny page area draws what fits and never panics
- **A whole-list load in progress**: only `Esc`/`q` act (0006), so no question opens
- **`[[actions]]` binding**: asks exactly as the popup does (the user's choice: a bound key is no reason to skip it)
- **A user keymap binding `d` to something else** while `RemoveFromQueue` is `d d`: 0008 refuses it (`d` is a prefix of `d d`) and names both; the user unbinds one

## Test plan

Each test is named after its criterion. Red is a failing assertion against stubs with stub bodies (the removals send at once as today, `question` returns an empty string, the config ignores the key, the default keymap keeps `d`), no `todo!()` and no compile errors.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/browse/tests.rs` :: `ac1_confirm_remove_favorite` (table: kind × popup/binding × `y`/`n`/`Esc`) | question text, nothing sent before `y`, the write after `y`, nothing after `n`/`Esc` | the write is sent at once, no popup |
| AC2 | `crates/core/src/ui/browse/tests.rs` :: `ac2_confirm_remove_from_playlist` | question, position and ETag kept across a page reload | the write is sent at once |
| AC3 | `crates/core/src/ui/browse/tests.rs` :: `ac3_confirm_remove_from_queue`; `crates/core/src/ui/filter/tests.rs` :: `ac3_confirm_remove_filtered_queue` | question, the entry asked about after cursor and snapshot changes, empty queue, filtered queue | the command is sent at once |
| AC4 | `crates/core/src/ui/browse/tests.rs` :: `ac4_confirm_removals_off` (table) | sends at once when off; delete and duplicate still ask | stub `State::confirm_removals` is read nowhere and the removals never ask: the "on" rows and the delete row pass, the test fails on the missing questions it expects with the setting on |
| AC5 | `crates/core/src/ui/popup.rs` (a new `#[cfg(test)] mod tests`) :: `ac5_removal_question` (table) | the texts for N = 1 and N = 3 | stub returns `""` |
| AC6 | `crates/core/src/ui/keymap/tests.rs` :: `ac6_remove_from_queue_is_d_d`; `crates/core/src/ui/browse/tests.rs` :: `ac6_d_alone_does_nothing`; `crates/app/tests/examples.rs` :: `ac18_example_keymap_is_the_defaults` (existing, fails until the example says `d d`) | default binding, `d` pending, `d d` asks, example | defaults keep `d` |
| AC7 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows); `crates/app/src/main.rs` :: `ac7_tui_state_confirm_removals`; `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` (existing) | value per source, error, model copy, example | stub config ignores the key |
| AC8 | `crates/app/src/ui.rs` :: `ac8_confirm_removal` (snapshot), `ac17_no_panic_any_size` (new row); `crates/app/tests/docs.rs` :: `ac8_confirm_removals_documented`; zh-TW copies, README and `CLAUDE.md` reviewed at acceptance | the drawn question; docs name `confirm_removals` and `d d` | no question drawn; docs lack the terms |

Existing tests that press `d` on the queue (`crates/core/src/ui/browse/tests.rs`, `filter/tests.rs`, `search/tests.rs`, `help/tests.rs`) and expect `RemoveFromQueue` at once are updated to this spec in the same change (`d d`, then `y`), never weakened.

Checked by hand at acceptance: in a real terminal, `d d` on the queue asks and `y` removes; `d` then `Esc` does nothing; *Remove from favorites* in the popup asks; `confirm_removals = false` removes at once.

## Crate placement

- `tidal-player-core::ui`: `Confirmed` gains `Remove(Removal)`, `Removal { from: RemoveFrom, items: Vec<Removed> }` with `RemoveFrom::{Favorites(FavoriteKind), Playlist { uuid, title, etag }, Queue}` and `Removed { title, target }` (`target`: the favorite's ID, the playlist position or the `EntryId`), and the pure `question(&Removal) -> String`; `State` gains `confirm_removals: bool` (default `true`); `keymap::defaults` binds `d d`. A removal opens the question through one function that the popup, the bindings and `RemoveFromQueue` share, so #71 adds a list of items, not a new path. No I/O, no new dependency
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

## Out of scope

- Multi-select and what `d` does with a selection (#71); sending several removals at once
- An undo (re-adding what was removed)
- Asking before `tidal-player logout` or a queue-clearing key
- `d d` on pages other than the queue (removing from favorites or a playlist by key, without the popup): `DeleteFromLiked`/`DeleteFromPlaylist` can be bound in `[[actions]]` today
