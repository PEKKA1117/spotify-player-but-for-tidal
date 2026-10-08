# 0012 — Keybindings help (`?`)

- **Status**: draft (2026-10-08)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0004 (implemented: the keys, the TUI), 0006 (implemented: pages, windows, popups, their keys)
- **User docs**: [`docs/tui.md`](../tui.md) "Keys" gains the help popup (AC6)
- **Numbering**: 0007–0011 are reserved (0001 "Out of scope"); this takes the help popup out of 0008, which keeps configurable keys

## Context

The TUI has some 40 keys, and which ones act depends on the page, the focused window and any open popup (0006). spotify-player shows its keys with `?`; the user asked for lazygit's version of it (2026-10-08): `?` opens a popup that lists **the keys of where you are first** (the focused view), then the global ones, and lets you filter the list and run a key from it.

Today the keys are a hard-coded `match` in `tidal_player_core::ui` (`key_press`, `browse_key`, the popup handlers), and `docs/tui.md` lists them by hand. A help screen written by hand a third time would drift the moment a key changes. So this spec first makes the bindings **one table** that the key handling, the help popup and a docs check all read. That table is also what 0008 needs to make keys configurable.

### What tidalt did

Read from tidalt's `internal/ui/help.go` and `keymap.go` (`f66b3f0`, `ee9c24e`): `?` opened a popup generated from the keymap (`helpRows`), every bound action under fixed groups, with spotify-player-style key labels. It was not aware of where you were: the same full list everywhere, including keys that did nothing in the current view, and no filter. So the generation from the keymap is kept; the context-first sections and the filter are new

### What could go wrong, and the criterion that covers it

1. **Help that lies**: a key listed that does nothing, or does something else, or a key that works but is not listed → help, dispatch and docs all come from one binding table, and a test runs every binding through dispatch (AC1, AC6)
2. **The key you want is buried**: forty keys in one flat list → the focused view's section comes first, named after it (AC2)
3. **`?` typed into a prompt opens help** → in text prompts (`o`/`O`, the playlist name) `?` types a `?` (AC3)

## Behaviour

### The binding table

Every key the TUI acts on is a **binding**: one or more keys (`j`, `↓`), an **action** (a named UI action: `Next`, `CursorDown`, `OpenLibrary`, `AddToQueue`, …), a short description (`next track`, `move down`), and the **contexts** it applies in. Contexts:

| Context | When |
|---|---|
| a popup (`Actions`, `Add to playlist`, `New playlist`, `Confirm`, `Role filter`, `Help`) | that popup is open: only its bindings act (0006 rule, unchanged) |
| a window (`Queue`, `Library · Playlists`, `Library · Albums`, `Library · Artists`, `Favorite tracks`, `Album`, `Playlist`, `Artist · Top tracks`, `Artist · Albums`, `Artist · Appears on`, `Artist · All tracks`) | that window has the focus |
| `Lists` | any list window (cursor keys, `Enter`, `Z`, actions) |
| `Pages` | always: open/close pages, history, focus |
| `Playback` | always (playback keys still do nothing on an empty queue, 0004) |
| `App` | always: help, quit |

Key handling looks the pressed key up in the table, most specific context first (popup, then window, then `Lists`, `Pages`, `Playback`, `App`), and runs the action. Two-key sequences (`g g`, `g l`, `g y`, `g a`) are bindings too. Behaviour of every key stays exactly as specified by 0004–0006: this is a refactor, and their tests stay green unchanged.

### The help popup

`?` opens it anywhere except while typing into a text prompt. Centred, 80 % of the page area (at least 40 × 10, clipped to the terminal), titled `Keybindings`:

```
┌Keybindings─────────────────────────────────────────────┐
│ Library · Playlists                                    │
│   Enter            open the playlist                   │
│   Z  Ctrl-z        add the playlist to the queue       │
│ Lists                                                  │
│   j  ↓             move down                           │
│   k  ↑             move up                             │
│   …                                                    │
│ Pages                                                  │
│   z                queue                               │
│   g l              library                             │
│ Playback                                               │
│   Space            play / pause                        │
│ App                                                    │
│   ?                this help                           │
│   q  Ctrl-c        quit                                │
│ / filter · Enter run · Esc close                       │
└────────────────────────────────────────────────────────┘
```

- **Sections**: the open popup's (when `?` is pressed inside a popup), then the focused window's, then `Lists` (on list windows), `Pages`, `Playback`, `App`. A section lists only the bindings of its context; a binding that would do nothing right now (playback keys on an empty queue, `d` off the queue) is drawn dim, not hidden
- **Moving**: `j`/`↓`, `k`/`↑`, `g g`, `G`, `Ctrl-f`/`PageDown`, `Ctrl-b`/`PageUp` move the highlight over the binding rows (section headings are skipped)
- **Filter** (lazygit's `/`): `/` starts it; typed characters filter the rows to those whose keys or description contain the text, case-insensitively; empty sections disappear; `Backspace` deletes; `Enter` ends typing and keeps the filter; `Esc` clears the filter
- **Running** (lazygit's `Enter`): `Enter` on a row closes the help and runs that binding's action, as if its key had been pressed in the view the help was opened from
- **Closing**: `Esc` (with no filter), `?` or `q` close it with no effect. While it is open no other key acts

## Acceptance criteria

- **AC1** — The binding table: `ui::bindings()` lists every binding; `key_press` and the popup handlers dispatch only through it. Table test over every binding × a state where its context is active: pressing the binding's first key gives the same effects and the same state as running its action directly. Every key of 0004–0006's key tests is in the table (their tests unchanged and green)
- **AC2** — Help contents (pure, `ui::help(&State) -> Vec<HelpSection>`), table over contexts: queue page; library with each of its three windows focused; favorite tracks; album; playlist (own and followed); artist with each of its four windows focused; each popup open. Asserts the section titles in order and that each section holds exactly the bindings of its context; dim rows where the action would do nothing (playback keys with an empty queue)
- **AC3** — Help keys (model, table): `?` opens it from every page and popup but types a `?` in the open prompt and the playlist-name prompt; movement keys move over binding rows only, clamped; `/` + typed text filters (on keys and descriptions, case-insensitive), `Backspace` edits, `Enter` keeps the filter, `Esc` clears it, a second `Esc` closes; `Enter` on a row closes help and emits exactly what pressing that key in the original view emits (e.g. `Space` → `Send(TogglePause)`, `g l` → the library page pushed and fetched); `?`/`q` close with no effect; no other key acts while open
- **AC4** — Rendering (`insta`, 80×24, reviewed by eye, + `contains` checks): help on the queue page; on the library with Albums focused; filtered by `fav`; opened from the actions popup. 40×12: the key column kept, descriptions truncated with `…`. No panic from 0×0 to 120×40
- **AC5** — Every key label in the table renders as the docs write it (`Ctrl-s`, `Shift-Tab`, `Space`, `↓`), from one formatter used by the help popup and the docs check
- **AC6** — Docs: `docs/tui.md` "Keys" documents `?` and the help popup; a test asserts that every binding's key label and description appear in `docs/tui.md` (so a new key without docs fails the build); `CLAUDE.md` "Status" names this spec

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| Terminal smaller than 40 × 10 | The popup takes what there is, clipped; never a panic (AC4) |
| Filter matches nothing | `No keys match "xyz"`; `Esc` clears it |
| `?` while a whole list loads (0006) | Opens; `Enter` on a row runs it only if it would act during the load (only `Esc`, `q`, `C-c` do) |
| `?` while disconnected (0005) | Opens; playback rows dim (they send nothing while disconnected) |
| A binding with several keys | All shown on its row (`j  ↓`); the filter matches any of them |

## Test plan

Red is a failing assertion against stub bodies (no `todo!()`, no compile errors), as in 0001–0006.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/keys.rs` :: `ac1_every_binding_dispatches` (table over `bindings()`) + the unchanged 0004–0006 key tests | key == action, same effects and state | stub `bindings()` is empty, so the coverage assertion (every key of the old tests is in the table) fails |
| AC2 | `crates/core/src/ui/help.rs` :: `ac2_help_sections` (table over contexts) | titles, order, membership, dim rows | stub `help` returns one flat `All` section |
| AC3 | `crates/core/src/ui/help.rs` :: `ac3_help_keys` (table) | open/close, movement, filter, run, key capture | stub `?` does nothing |
| AC4 | `crates/app/src/ui.rs` :: `ac4_help_80x24`, `ac4_help_40x12`, `ac4_help_no_panic` | `contains` + snapshots | stub render draws no popup |
| AC5 | `crates/core/src/ui/keys.rs` :: `ac5_key_labels` (table) | labels | stub labels with `Debug` (`Ctrl('s')`) |
| AC6 | `crates/app/tests/docs.rs` :: `ac6_every_binding_documented` + reviewed at acceptance | every label and description in `docs/tui.md` | stub docs lack `?` |

## Crate placement

- `tidal-player-core::ui`: `keys` (the table, `UiAction`, contexts, labels, dispatch), `help` (sections, filter, the help popup's state and keys). No I/O, no new dependency
- `tidal-player`: rendering the popup (`ui.rs`), the docs test
- `xtask layering`: no change

## Decisions (to be answered by the user before approval)

1. **One binding table, with key handling refactored onto it** (*proposed*), so help cannot drift; the alternative, a hand-written help list checked by a test against dispatch, is smaller now but is redone by 0008
2. **`Enter` runs the binding** (lazygit, *proposed*), or the popup only shows keys
3. **Rows that would do nothing now**: drawn dim (*proposed*) or hidden
4. **Number**: 0012 (*proposed*, keeps 0007–0011 as planned) or take 0007 and move search to 0012
5. **PR**: this spec rides PR #8 (spec 0006, not merged yet) or waits for it to merge and gets its own PR (*proposed*: its own, per "one spec → one PR")

## Out of scope

- Configurable keys and `keymap.toml` (0008; the table here is what it will load into)
- Mouse support, clickable help rows
- Help for the one-shot `playback` command line (`--help` covers it)
