# 0013 — Key-sequence hints

- **Status**: implemented (2026-10-09; the manual checks under "Test plan" are run on the user's machine)
- **Owner**: tech-lead (primary session)
- **Issue**: [#17](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/17)
- **Depends on**: 0004 (implemented: the screen), 0008 (implemented: the keymap, key sequences, the keys help and its texts, `app.toml`)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Key hints"; [`docs/config.md`](../config.md) gains `key_hints` and `key_hints_delay_ms` (AC7)

## Context

Six default bindings are two-key sequences starting with `g` (`g g`, `g a`, `g l`, `g y`, `g s`, `g m`), and `keymap.toml` can add sequences of any length (0008, "Key sequences and where keys act"). After the first key nothing on screen says a sequence is pending or what can follow it: the user has to remember the second key or open the keys help (`?`), which also clears the pending keys. which-key (Neovim) and Helix show a small popup listing the keys that can follow a prefix. This spec adds one.

tidalt had no multi-key bindings, so there is nothing to carry over.

## Behaviour

### When the hint shows

When a key sequence has been pending for the **delay** (`key_hints_delay_ms`, default 1000 ms) without a further key (the keys pressed so far start a longer binding but are no binding themselves, 0008's rule), and at least one of the bindings it starts **acts where the user is**, a **hint** box is drawn. A sequence typed faster than the delay never shows one. Each further key that keeps a sequence pending (`s` then `l` of `s l a`) restarts the delay. The hint goes away as soon as nothing is pending:

- the next key completes a binding (that binding runs, as today)
- the next key starts nothing (collecting restarts with that key alone, as today: `g x` does what `x` does, `g esc` does what `esc` does, so `esc` cancels)
- something else clears the pending keys (a text input or a popup's fixed key, opening the keys help)

It is not drawn while the keys help is open (the help is modal and already lists every key), during a whole-list load (only `esc`/`q` act there), when the page area is not drawn (terminals under 8 rows, 0004), or when `key_hints = false`.

The hint does not change what keys do: dispatch reads the same pending keys whether a hint is drawn or not, and the delay only decides when the box is drawn (0008: sequences still have no timeout). The delay is measured by the TUI (it redraws at least every 100 ms), so the box appears within one frame after the delay.

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

`app.toml` gains, read by the TUI only (as `volume_step`: the daemon accepts and ignores them, and each attached TUI uses its own, 0008 AC16's rule for the keymap):

| Key | Accepted | Default | Overridden by |
|---|---|---|---|
| `key_hints` | `true`, `false` | `true` | `TIDAL_PLAYER_KEY_HINTS` (`on`, `off`) |
| `key_hints_delay_ms` | integer 0–10 000 (`0`: at once) | `1000` | `TIDAL_PLAYER_KEY_HINTS_DELAY_MS` |

The delay is in milliseconds, not 0008's `_secs`, because useful values are below a second.

## Acceptance criteria

- **AC1** — With a sequence pending, the model's hints (`help::hints(&State) -> Option<Hints>`) give the pending keys' label and one entry per next key: the next key's label, the keys help's text, dim as in the keys help, in the keys help's order, only bindings the keys help lists in this view. With the default keymap after `g`: on a library window (`a`, `g`, `l`, `y`, `s`, `m`), on the queue page (`a` reads "actions on the entry"), over the actions popup (`g` only). A custom keymap's bindings appear and an unbound one is gone
- **AC2** — A next key that starts longer sequences gives one entry with `+N` and no text, dim only when everything under it is; after it is pressed the hints list the next level
- **AC3** — No hints when: nothing is pending; the keys help is open; a whole-list load is shown; `key_hints` is off; the pending keys start only bindings that do not act in this view
- **AC4** — The hint never changes what a key does: for a table of key sequences (complete `g l`, mismatch `g x`, cancel `g esc`, nested `s l a`, a sequence over a popup), the effects and the resulting state (hints aside) are the same with `key_hints` on and off, and the hints are gone once the sequence completes, mismatches or is cancelled
- **AC5** — Delay: the TUI's hint timer (`HintTimer`, fed the pending keys and the time after every update) says to draw the hint only once the same pending keys have been pending for the delay: not before (999 ms of 1000), yes at and after it, at once with a delay of 0, never with no pending keys; a further key that keeps a sequence pending restarts it; completing, cancelling or mismatching resets it
- **AC6** — Rendering: the hint box is docked at the bottom of the page area, titled `g …`, with entries in columns as above; dim entries are dim; it truncates with `…` and `… +N more`; no hint under 3 × 12 or with no page area; drawing it at any terminal size from 1 × 1 to 200 × 60 does not panic. Snapshots at 80 × 24 (library, queue, actions popup), 40 × 12, 120 × 30 and with a nested prefix
- **AC7** — `key_hints` and `key_hints_delay_ms` in `app.toml`, `TIDAL_PLAYER_KEY_HINTS` and `TIDAL_PLAYER_KEY_HINTS_DELAY_MS` set the TUI's settings with 0008's precedence and errors (a non-boolean `key_hints`, a delay out of 0–10 000: an error naming the key; exit 2); `examples/app.toml` lists both with their defaults; `docs/tui.md`, `docs/config.md` and their zh-TW copies describe the hint and the setting

## Edge cases & errors

- **A custom keymap binds a prefix alone** (`g` to a command): 0008 refuses it unless the longer sequences are unbound first, so a bound key never shows a hint
- **No entry acts here** (e.g. a custom `x y` bound to `RemoveFromQueue`, `x` pressed on a library page): no hint is drawn, and the pending keys behave as today
- **Very many entries** (a custom keymap with 30 bindings under one prefix): `… +N more` in the last cell; the keys help (`?`) lists them all
- **Delay while the terminal is idle**: the main loop ticks every 100 ms without input, so the box appears without a key press
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
| AC5 | `crates/app/src/ui.rs` :: `ac5_hint_timer` (table: key presses and instants → draw or not; fake instants, no sleeping) | delay, restart, reset, delay 0 | stub timer always says draw |
| AC6 | `crates/app/src/ui.rs` :: `ac6_key_hints` (snapshot per row), `ac6_key_hints_layout` (table: entries × area → columns, rows, `… +N more`), `ac17_no_panic_any_size` (new rows with a pending `g`) | position, columns, title, dim, truncation, sizes | stub render draws no hint |
| AC7 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows), `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` (unchanged, fails until the example has the key), `crates/app/tests/docs.rs` :: `ac7_key_hints_documented` + zh-TW copies reviewed at acceptance | value per source, error message, example, docs mention both keys | stub config ignores both keys |

Checked by hand at acceptance: in a real terminal `g l` typed quickly opens the library with no box; `g` held for a second shows it and `l` then opens the library; `key_hints = false` hides it.

## Crate placement

- `tidal-player-core::ui`: `help::hints(&State) -> Option<Hints>` (content only: no time) (`Hints { prefix: String, entries: Vec<HintEntry> }`, `HintEntry { key: String, text: String, dim: bool, more: usize }`), built from `help::help` so the context and text rules stay in one place; `State` gains `key_hints: bool` (default `true`). No I/O, no new dependency
- `tidal-player`: config (both keys and variables), `HintTimer` in `ui.rs` (pending keys + an injected `Instant` → draw or not; the main loop feeds it `Instant::now()` after each batch of updates and passes its answer to `render`), the pure layout function (entries and an area → the box and its cells) and the drawing in `ui/pages.rs`
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-09, from code): pending keys live in `State::pending` (`crates/core/src/ui/dispatch.rs`: `resolve`, `set_pending`); fixed keys and opening the help clear them; the keys help's sections, texts and dim rule are `help::help`, `text`, `dim` (`crates/core/src/ui/help.rs`); the default multi-key bindings are the six `g` ones (`keymap::defaults`); 0008 refuses a bound prefix of a longer sequence (`check_prefixes`); the page area is absent under 8 terminal rows (`crates/app/src/ui.rs` `areas`).

Assumptions: none about external systems (no API, no audio, no terminal protocol involved).

## Decisions (answered by the user, 2026-10-09)

1. **Delay**: *answered: default 1 s*, configurable (`key_hints_delay_ms`, `0` for at once), measured by the TUI, not the core model (it has no clock)
2. **Off switch**: *answered: yes*, `key_hints` in `app.toml` (+ environment), default on
3. **Position**: *answered: as proposed*: docked at the bottom of the page area
4. **Context**: *answered: only meaningful ones*: only bindings that act in this view (the keys help's rule)

## Out of scope

- Hints for single keys (a hint after every key), or for the text inputs' fixed keys
- Showing the pending keys anywhere else (the playback window's header)
- Mouse support on the hint; scrolling a hint that does not fit (the keys help does that)
- Filter and sort (0012, issue #18)

## Implementation notes (choices made where the spec was silent, 2026-10-09)

- **A 3-row page area**: the box may take at most the page area's height less one row, so at exactly 3 rows it would be its two borders with no entry; no hint is drawn there either (the page area has 2 rows at the smallest terminal that draws one, 8 rows, so the hint needs a terminal of at least 10 rows)
- **Drawing**: the title is bold, as the keys help's; the `… +N more` cell is plain (not dim). The hint is drawn after an open popup (over it, as the 80 × 24 actions-popup snapshot shows) and before the keys help (which suppresses it anyway). An entry is `key  text` and a nested prefix `key  +N`; both are cut to the column with `…`
- **Wiring**: `render(state, frame, hint)` takes the timer's answer; the main loop builds a `HintTimer` from `key_hints_delay_ms` and feeds it `State::pending` and `Instant::now()` after each batch of updates (one terminal event per batch, so a cancelled and restarted `g` is seen as a reset). `PlayerSettings` carries `key_hints` and `key_hints_delay` (resolved like `volume_step`; the daemon resolves and ignores them); `tui_state` copies `key_hints` into `State` for both the standalone and the attached TUI (`ac7_tui_state_key_hints` in `crates/app/src/main.rs` checks it)
- **Nested prefix counting**: `+N` counts the distinct listed bindings under the next key (a binding with two sequences under it counts once); a nested entry takes the place of the first listed binding it leads to in the keys help's order
- **Red stub**: one stub served all four core tests: `hints` returned every binding under the prefix whatever the state (AC3's "Expected red"), so AC1 and AC2 failed on a wrong `Some` (no texts, no `+N`) rather than on `None`
- **Docs**: "Key hints" is a `###` section under "Keys" in `docs/tui.md`; `docs/playback.md`'s settings table (and its zh-TW copy) also lists both keys. `ac7_key_hints_documented` checks the English and zh-TW `config.md` tables and both `tui.md` copies
