# 0013 — Key-sequence hints

- **Status**: draft
- **Owner**: tech-lead (primary session)
- **Issue**: [#17](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/17)
- **Depends on**: 0004 (implemented: the screen), 0008 (implemented: the keymap, key sequences, the keys help and its texts, `app.toml`)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Key hints"; [`docs/config.md`](../config.md) gains `key_hints` (AC6)

## Context

Six default bindings are two-key sequences starting with `g` (`g g`, `g a`, `g l`, `g y`, `g s`, `g m`), and `keymap.toml` can add sequences of any length (0008, "Key sequences and where keys act"). After the first key nothing on screen says a sequence is pending or what can follow it: the user has to remember the second key or open the keys help (`?`), which also clears the pending keys. which-key (Neovim) and Helix show a small popup listing the keys that can follow a prefix. This spec adds one.

tidalt had no multi-key bindings, so there is nothing to carry over.

## Behaviour

### When the hint shows

While a key sequence is pending (the keys pressed so far start a longer binding but are no binding themselves, 0008's rule) and at least one of the bindings it starts **acts where the user is**, a **hint** box is drawn. It goes away as soon as nothing is pending:

- the next key completes a binding (that binding runs, as today)
- the next key starts nothing (collecting restarts with that key alone, as today: `g x` does what `x` does, `g esc` does what `esc` does, so `esc` cancels)
- something else clears the pending keys (a text input or a popup's fixed key, opening the keys help)

It is not drawn while the keys help is open (the help is modal and already lists every key), during a whole-list load (only `esc`/`q` act there), when the page area is not drawn (terminals under 8 rows, 0004), or when `key_hints = false`.

The hint does not change what keys do: dispatch reads the same pending keys whether a hint is drawn or not, and nothing waits on a timer (0008: sequences have no timeout).

### What it lists

One **entry** per distinct next key, built from the live keymap (`keymap.toml` included) and filtered to the current context with the same rules as the keys help (0008 "The keys help"):

- the entries are the keys help's rows for the current view (its window or popup section, `Lists`, `Pages`, `Playback`, `Actions`, `App`, without a filter) whose key sequences start with the pending keys and are longer than them, in the keys help's order. A binding the help does not list here (it does nothing in this view) has no entry
- an entry shows the **next key** in the file's syntax (`l`, `C-x`, `enter`) and the binding's **help text** from the keys help (`the library`, `move to the top`); a binding with several sequences under the prefix gives one entry per next key
- when the next key itself starts longer sequences (a **nested prefix**, e.g. `s l a` and `s l b` after `s`), the entry shows the next key and `+N`, N being how many listed bindings it leads to (`l  +2`). The next level is listed after that key is pressed
- an entry whose binding would do nothing right now (the keys help's dim rule: playback keys on an empty queue, row keys without a row, player commands while disconnected) is drawn dim. A nested-prefix entry is dim only when everything it leads to is

The default keymap after `g`, on a library window with a row selected, at 80 × 24 (columns are capped at 32, so two fit in the 76 inner columns and the longest text is cut):

```
│┌g …─────────────────────────────────────────────────────────────────────────┐│
││a  actions on the selected row     y  favorite tracks                       ││
││g  move to the top                 s  the search page (on one: its…         ││
││l  the library                     m  your mixes                            ││
│└────────────────────────────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────────────────────┘
```

### Where and how it is drawn

- **Docked** at the bottom of the page area, over the page's last rows, the full width of the page area, like a popup (the page is not resized or reflowed, and its list height, `Resize`'s rows, is unchanged)
- A **bordered box** titled with the pending keys and `…` (`g …`, `s l …`)
- **Columns**: entries fill column by column (top to bottom, then the next column), in the order above. Each entry is `key  text` (two spaces). Every column is as wide as the widest entry, at most 32 cells (and at most the box's inner width), with three spaces between columns; as many columns as fit, and as few rows as that needs
- **Height**: the rows plus the two border rows, at most the page area's height less one row, so the page's title row stays visible. When the entries need more rows than that, the last cell reads `… +N more` (N entries not shown)
- **Narrow**: a text that does not fit its column is cut with `…`. A page area under 3 rows or under 12 columns draws no hint

### Config

`app.toml` gains `key_hints` (`true`/`false`, default `true`), overridden by `TIDAL_PLAYER_KEY_HINTS` (`on`, `off`), read by the TUI only (as `volume_step`): the daemon accepts it and ignores it, and each attached TUI uses its own (0008 AC16's rule for the keymap).

## Acceptance criteria

- **AC1** — With a sequence pending, the model's hints (`help::hints(&State) -> Option<Hints>`) give the pending keys' label and one entry per next key: the next key's label, the keys help's text, dim as in the keys help, in the keys help's order, only bindings the keys help lists in this view. With the default keymap after `g`: on a library window (`a`, `g`, `l`, `y`, `s`, `m`), on the queue page (`a` reads "actions on the entry"), over the actions popup (`g` only). A custom keymap's bindings appear and an unbound one is gone
- **AC2** — A next key that starts longer sequences gives one entry with `+N` and no text, dim only when everything under it is; after it is pressed the hints list the next level
- **AC3** — No hints when: nothing is pending; the keys help is open; a whole-list load is shown; `key_hints` is off; the pending keys start only bindings that do not act in this view
- **AC4** — The hint never changes what a key does: for a table of key sequences (complete `g l`, mismatch `g x`, cancel `g esc`, nested `s l a`, a sequence over a popup), the effects and the resulting state (hints aside) are the same with `key_hints` on and off, and the hints are gone once the sequence completes, mismatches or is cancelled
- **AC5** — Rendering: the hint box is docked at the bottom of the page area, titled `g …`, with entries in columns as above; dim entries are dim; it truncates with `…` and `… +N more`; no hint under 3 × 12 or with no page area; drawing it at any terminal size from 1 × 1 to 200 × 60 does not panic. Snapshots at 80 × 24 (library, queue, actions popup), 40 × 12, 120 × 30 and with a nested prefix
- **AC6** — `key_hints` in `app.toml` and `TIDAL_PLAYER_KEY_HINTS` set the TUI's state with 0008's precedence and errors (a non-boolean value is an error naming the key; exit 2); `examples/app.toml` lists it with its default; `docs/tui.md`, `docs/config.md` and their zh-TW copies describe the hint and the setting

## Edge cases & errors

- **A custom keymap binds a prefix alone** (`g` to a command): 0008 refuses it unless the longer sequences are unbound first, so a bound key never shows a hint
- **No entry acts here** (e.g. a custom `x y` bound to `RemoveFromQueue`, `x` pressed on a library page): no hint is drawn, and the pending keys behave as today
- **Very many entries** (a custom keymap with 30 bindings under one prefix): `… +N more` in the last cell; the keys help (`?`) lists them all
- **Resize while pending**: the next frame lays the hint out for the new size
- **Attached TUI**: hints are computed from that client's keymap and `key_hints`; nothing goes over the socket
- **Connection lost while pending**: the dim rule follows the connection, as in the keys help

## Test plan

Each test is named after its criterion. Red is a failing assertion against stub types and functions with stub bodies (`hints` returns `None`, the render draws nothing, the config ignores the key), no `todo!()` and no compile errors, as in 0001–0011.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/help/tests.rs` :: `ac1_hints_entries` (table: view × keymap → title, keys, texts, dim) | entries, order, texts, context, custom keymap | stub `hints` returns `None` |
| AC2 | `crates/core/src/ui/help/tests.rs` :: `ac2_hints_nested_prefix` | `+N` entry, its dim, next level after the key | stub `hints` returns `None` |
| AC3 | `crates/core/src/ui/help/tests.rs` :: `ac3_no_hints` (table) | `None` in each listed case | stub `hints` returns `Some` with every binding under the prefix whatever the state |
| AC4 | `crates/core/src/ui/help/tests.rs` :: `ac4_hints_do_not_change_keys` (table) | same effects and state on/off; hints gone afterwards | stub `hints` ignores `pending` and keeps returning entries after the sequence ends |
| AC5 | `crates/app/src/ui.rs` :: `ac5_key_hints` (snapshot per row), `ac5_key_hints_layout` (table: entries × area → columns, rows, `… +N more`), `ac17_no_panic_any_size` (new rows with a pending `g`) | position, columns, title, dim, truncation, sizes | stub render draws no hint |
| AC6 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows), `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` (unchanged, fails until the example has the key), `crates/app/tests/docs.rs` :: `ac6_key_hints_documented` + zh-TW copies reviewed at acceptance | value per source, error message, example, docs mention `key_hints` | stub config ignores `key_hints` |

Checked by hand at acceptance: `g` in a real terminal shows the box and `l` opens the library; `key_hints = false` hides it.

## Crate placement

- `tidal-player-core::ui`: `help::hints(&State) -> Option<Hints>` (`Hints { prefix: String, entries: Vec<HintEntry> }`, `HintEntry { key: String, text: String, dim: bool, more: usize }`), built from `help::help` so the context and text rules stay in one place; `State` gains `key_hints: bool` (default `true`). No I/O, no new dependency
- `tidal-player`: config (`key_hints`, `TIDAL_PLAYER_KEY_HINTS`), the pure layout function (entries and an area → the box and its cells) and the drawing in `ui/pages.rs`
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-09, from code): pending keys live in `State::pending` (`crates/core/src/ui/dispatch.rs`: `resolve`, `set_pending`); fixed keys and opening the help clear them; the keys help's sections, texts and dim rule are `help::help`, `text`, `dim` (`crates/core/src/ui/help.rs`); the default multi-key bindings are the six `g` ones (`keymap::defaults`); 0008 refuses a bound prefix of a longer sequence (`check_prefixes`); the page area is absent under 8 terminal rows (`crates/app/src/ui.rs` `areas`).

Assumptions: none about external systems (no API, no audio, no terminal protocol involved).

## Decisions (to be answered by the user)

1. **Delay**: *proposed*: none; the hint shows on the first key. Neovim's which-key waits (`timeoutlen`), Helix does not. A delay needs a timer in the UI loop for a sequence that has none. Alternative: `key_hints_delay_ms`
2. **Off switch**: *proposed*: `key_hints` in `app.toml` (+ environment), default on. Alternative: always on, no setting
3. **Position**: *proposed*: docked at the bottom of the page area (the playback window is at the top here, so "above the now-playing bar" from the issue becomes "at the bottom of the screen", where Helix puts it). Alternative: bottom-right corner box, Helix's exact placement
4. **Context**: *proposed*: only bindings that act in this view (the keys help's rule), so `g a` is not offered over a popup. Alternative: every binding under the prefix, those that do nothing here dim

## Out of scope

- Hints for single keys (a hint after every key), or for the text inputs' fixed keys
- Showing the pending keys anywhere else (the playback window's header)
- Mouse support on the hint; scrolling a hint that does not fit (the keys help does that)
- Filter and sort (0012, issue #18)
