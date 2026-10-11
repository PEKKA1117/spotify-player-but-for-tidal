# 0016 — Album cover in the playback window

- **Status**: implemented (2026-10-12; approved 2026-10-12)
- **Owner**: tech-lead (primary session)
- **Issue**: [#19](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/19)
- **Depends on**: 0004 (implemented: the screen and its playback window), 0005 (implemented: attached TUIs), 0008 (implemented: `app.toml` and its precedence), 0010 (implemented: `AlbumRef::cover`, `cover_url`, the cover cache and `max_cover_arts`)
- **User docs**: [`docs/tui.md`](../tui.md) "Playback window" gains "Album cover"; [`docs/config.md`](../config.md) gains `cover_img`, `cover_img_rows` and `cover_img_protocol`; [`docs/mpris.md`](../mpris.md) "Album covers" (cached with MPRIS off too); their zh-TW copies (AC10)

## Context

The playback window (0004) shows the current track as four lines of text. spotify-player draws the album cover left of them, and tidalt drew one too (Kitty graphics, later sixel: `internal/ui/coverart.go`, `becf508`). 0010 already carries the cover ID with every track, builds its URL (`cover_url`) and keeps a cover cache for MPRIS, and left the TUI cover to this spec.

Terminals draw images through one of three protocols: **sixel** (foot, WezTerm, xterm with `-ti vt340`, mlterm, Konsole), the **kitty** graphics protocol (kitty, Ghostty, WezTerm in part) and the **iTerm2** inline-image protocol (iTerm2, WezTerm). Many terminals support none (GNOME Terminal and other VTE terminals without sixel built in, the Linux console). The `ratatui-image` crate (11.1, for ratatui 0.30) detects which one a terminal answers to, encodes an image for it, and passes it through tmux.

## Behaviour

### When there is a cover

The cover is **active** when `cover_img` is on, the terminal has a graphics protocol (sixel, kitty, iTerm2, detected or forced, "Protocol" below), and the terminal is large enough ("Layout"). While it is active the playback window is **`cover_img_rows` rows tall** (default 6) instead of 4, and the cover area is reserved whatever plays, so the layout does not jump between tracks or while a cover loads:

- the current entry has a cover and it is ready: the cover is drawn
- it is loading, failed to load, the track has no cover (`album.cover` is none), or nothing is current: the cover area is blank

Not active: the playback window is exactly as today (4 rows, no reserved area), and the page below it is as tall as today.

### Layout

Inside the frame, with the cover active:

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│████████████ ▶ Hell Above · Pierce The Veil        shuffle  repeat: queue  80%│
│████████████   Collide With The Sky                                           │
│████████████   LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: …    │
│████████████                                                                  │
│████████████                                                                  │
│████████████   ━━━━━━━━━━━━━━━━━━━───────────────────────────────  1:23 / 3:32│
│┌Queue (12)──────────────────────────────────────────────────────────────────┐│
```

At 80 × 24 with `cover_img_rows = 6` and a 10 × 20 px cell: the cover (`█`) is 12 × 6, the text 65 columns wide. The details row is cut by 0004's rule (the snapshots under "Test plan" fix the exact text).

- **Cover area**: at the playback window's top-left, `cover_img_rows` rows tall and **`cols` columns wide**, `cols` being the width that makes it square in pixels: `round(rows × cell height / cell width)`, from the terminal's cell size in pixels (the protocol query gives it); with no cell size known, 2 columns per row (`12` for 6 rows). The cover is scaled to fit the area keeping its aspect (Tidal's covers are square), centred
- **Text**: one blank column after the cover, then the playback window's lines as today, in the remaining width (`width − cols − 1`): the header (`▶ Title · Artists` and the indicators), the album, the details or the message on the first three rows, the progress bar on the **last** row; the rows between are blank. Every rule of 0004 about cutting the text applies to that narrower width
- **Too small**: the cover is not active (the window is 4 rows, as today) when the frame's inner area is under `cover_img_rows + 2` rows (so the page keeps 2 rows, 0004's smallest) or under `cols + 1 + 20` columns (0004's narrowest title side, `MIN_LEFT`). It becomes active again as soon as a resize gives it room
- **Page height**: the page area starts below the taller window, and the rows a list shows (`Resize`, 0006) are counted with it

Popups, the actions menu and the key hints are drawn in the page area (0004–0013) and never over the playback window. The keys help over the whole frame only appears when there is no page area, where the cover is not active. If anything is ever drawn over the cover area, the cover is not drawn in that frame.

### Protocol

At start, once the terminal is in raw mode and on the alternate screen and before it reads the first event, a TUI with `cover_img` on asks the terminal which protocol it supports and its cell size in pixels (`ratatui-image`'s `Picker::from_query_stdio`, which waits at most its timeout, 2 s, for the answer). With `cover_img` off it sends nothing. `cover_img_protocol` decides:

| `cover_img_protocol` | Protocol used |
|---|---|
| `auto` (default) | the detected one: sixel, kitty or iTerm2; **none** when the terminal answered with none of them (no fallback to text blocks), or did not answer |
| `sixel`, `kitty`, `iterm2` | that one, whatever the terminal answered (for a terminal whose answer is wrong); the cell size from the query when it answered, else 2 columns per row |
| `halfblocks` | coloured `▀` cells: a low-resolution cover that works in any colour terminal |

Under tmux (recognised by `ratatui-image` from `TERM` starting with `tmux` or `TERM_PROGRAM=tmux`, not from `$TMUX`) the image is wrapped in tmux's passthrough. The crate switches passthrough on for the TUI's own pane at start (`tmux set -p allow-passthrough on`); `set -g allow-passthrough on` in `tmux.conf` is only needed when that fails (a tmux too old for per-pane options, for instance), and is documented. The protocol is the outer terminal's. A protocol that turns out not to work (the terminal draws garbage or nothing) is the user's to override with `cover_img_protocol` or `cover_img = false`: the TUI cannot see what the terminal drew.

### Where the image comes from

**The TUI never downloads a cover.** The player is the only process that downloads covers: it puts the current entry's cover in the cover cache (0010 "The cover cache"), and the TUI only reads files from that cache. The TUI and its player always share a machine and a user (standalone: one process; attached: 0005's Unix socket), so they share the cache directory, and the protocol between player and clients is unchanged.

- **The player caches whenever the cache is on**: 0010 downloaded the current cover only while MPRIS was on and available. With this spec the player's cover worker runs whenever `max_cover_arts > 0`, whether MPRIS is on, off (`mpris = false`) or unavailable (no session bus), so an attached TUI gets covers from any daemon. The worker's rules are 0010's (newest wins, 10 s, 5 MB, `.tmp` and rename, the limit with the current cover kept, one `warn` per failed cover); MPRIS, when on, takes its `file://` URL from the same worker
- When the current entry's cover changes (another entry becomes current, or the queue empties), the TUI's **cover loader** looks for `<cache dir>/covers/<cover>.jpg`. It never writes to the cache, touches a file or applies the limit (the player does, 0010)
- **Not there yet** (the player is still downloading it): the loader looks again every second while that cover is current, and stops when the file is there or the cover is no longer current. A file is only ever complete (the player renames it into place)
- **Never there** (the player's download failed, `max_cover_arts = 0`, the player uses another cache directory): the area stays blank; the loader keeps looking once a second, which costs one `stat`
- The image is read and decoded (JPEG) and encoded for the protocol at the cover area's size on the loader's thread; the UI only draws a ready image. A resize that changes the cover area's size has the image encoded again; until then the area is blank
- A result for a cover that is no longer current is dropped
- **Failures** (a file that cannot be read, an image that does not decode): the area stays blank for that entry, logged at `warn` once per cover, tried again only when that cover becomes current again. Never the player's message or the status line

### Config

`app.toml` gains, read by the TUI only (as `key_hints`: the daemon accepts and ignores them, and each attached TUI uses its own):

| Key | Accepted | Default | Overridden by |
|---|---|---|---|
| `cover_img` | `true`, `false` | `true` | `TIDAL_PLAYER_COVER_IMG` (`on`, `off`) |
| `cover_img_rows` | integer 4–20 | `6` | `TIDAL_PLAYER_COVER_IMG_ROWS` |
| `cover_img_protocol` | `auto`, `sixel`, `kitty`, `iterm2`, `halfblocks` | `auto` | `TIDAL_PLAYER_COVER_IMG_PROTOCOL` |

`max_cover_arts` stays the player's (0010): `0` turns the cache off, and with it the TUI's cover (the area is blank). The cache directory is resolved the same way in both (`$TIDAL_PLAYER_CACHE_DIR`, `$XDG_CACHE_HOME/tidal-player`, `~/.cache/tidal-player`); a TUI that resolves another one than its daemon shows no covers.

## Acceptance criteria

Pure logic (`tidal-player`):

- **AC1** — Layout (`ui::cover_layout`, table: frame inner size × `cover_img_rows` × cell size × active → playback window rect, cover rect, text rect, page rect): 78 × 22 inner (an 80 × 24 terminal), 6 rows, cell 10 × 20 px → cover 12 × 6 at the top-left, text from column 13 with width 65, page from row 6 with 16 rows; cell 8 × 18 px → `round(6 × 18 / 8)` = 14 columns; no cell size → 12; 4 rows → 8 columns; under `rows + 2` rows or under `cols + 21` columns, or not active → today's 4-row layout with no cover rect
- **AC2** — `ui::list_height` counts the taller window: with the cover active the list has `cover_img_rows − 4` rows fewer than without; under the size limits it is today's
- **AC3** — Protocol choice (`cover::protocol`, table: `cover_img` × `cover_img_protocol` × the query's answer (sixel, kitty, iTerm2, halfblocks only, no answer) → protocol or none, and whether the terminal is queried at all): `auto` takes sixel/kitty/iTerm2 and gives none for halfblocks or no answer; a forced protocol wins over any answer; `halfblocks` gives halfblocks; `cover_img = false` gives none and **no query**
- **AC4** — The cover loader's decisions (`cover::Slot`, pure state, table of events: the current cover changes, a result arrives, the area is resized, the queue empties, `cover_img` inactive): which cover to look for (the newest only), when to look again (every second while the current cover's file is missing, never after the cover stopped being current), which result to keep (the current cover at the current size) and which to drop (stale cover, stale size), when the area is blank, and that a cover that failed to decode is not tried again until it becomes current again

Rendering (`tidal-player::ui`, `insta`, `TestBackend`: the image itself is behind a seam and the snapshots show the reserved area blank):

- **AC5** — With the cover active the playback window is `cover_img_rows` rows, the text starts after the cover area and one blank column, the progress bar is on its last row, and the rest of 0004's text rules hold at the narrower width. Snapshots: playing at 80 × 24 (6 rows), paused at 120 × 30 (8 rows), nothing playing at 80 × 24 (area blank, window still tall), a message at 80 × 24, 40 × 12 (active, 25-column text side), 34 × 12 (too narrow: today's window)
- **AC6** — Not active (`cover_img = false`, protocol none, too small): every existing snapshot of 0004–0014 is unchanged (no snapshot is re-accepted by this spec). Drawing at every terminal size from 1 × 1 to 200 × 60 with the cover active does not panic (0004's no-panic test, new rows)
- **AC7** — The seam: `render` draws the ready image (a fake protocol recording the rect it was given) exactly in the cover rect, and nothing when the image's size is not the cover rect's (after a resize) or there is no ready image

I/O (`tidal-player`, temp dir, `wiremock`, as 0010 AC21):

- **AC8** — The TUI's loader (temp cache directory, a mock image server that must see **no request**: `wiremock` `expect(0)`): a cached cover is read, decoded and yields an image; a missing cover yields nothing, then yields once the file is renamed into place (fake clock, the one-second look); the cache directory's files, names and modification times are the same after the loader ran as before; a file that does not decode → blank, one `warn`; a newer cover while one is read → the older result is dropped
- **AC11** — The player caches without MPRIS: a daemon (0010's `tests/mpris.rs` harness, mock image server) with `mpris = false`, and one with no session bus, writes `covers/<cover>.jpg` for the current entry; with `max_cover_arts = 0` it writes nothing and makes no request. 0010's AC20/AC21 tests stay green unchanged
- **AC9** — Config: `cover_img`, `cover_img_rows`, `cover_img_protocol` in 0008's `app.toml` and precedence tables (defaults; `cover_img_rows = 3` and `21` errors naming the key; `cover_img_protocol = "ascii"` an error listing the accepted values; each variable over the file; an invalid variable an error; exit 2); the daemon accepts and ignores them; `examples/app.toml` lists the three with their defaults (0008's example test)

Docs:

- **AC10** — `docs/tui.md` "Playback window" (an "Album cover" section: when it shows, the protocols and which terminals, tmux's `allow-passthrough`, forcing or turning it off, the cache), `docs/config.md` (the three keys), `docs/mpris.md` "Album covers" (the player caches covers with MPRIS off too), their zh-TW copies, `README.md`/`README.zh-TW.md`, `CLAUDE.md` "Features", and 0010's "Out of scope" and "The cover cache" (*When*) pointing here. A test checks the three keys appear in both `config.md` copies and "allow-passthrough" in both `tui.md` copies (0013 AC7's test, new rows)

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| Terminal answers the query with nothing, or slowly | Waits at most `ratatui-image`'s timeout (2 s) once at start, then no cover (`auto`) |
| Terminal answers without its cell size (xterm with window ops disallowed) | `ratatui-image` falls back to halfblocks, so no cover under `auto`; documented: force `cover_img_protocol = "sixel"` |
| `cover_img = false` | No query is sent; the TUI is exactly as before |
| Not a terminal that answers queries (e.g. a serial console, `TERM=linux`) | No answer → none |
| tmux refuses the per-pane `allow-passthrough` | tmux drops the image; the area stays blank. Documented (`set -g allow-passthrough on`); `cover_img = false` hides the reserved area |
| tmux attached from two different terminals | The protocol is the one detected at the TUI's start |
| Resize | The next frame lays out for the new size; the image is encoded again for a new cover size, blank until then |
| The terminal becomes too small | The cover is not active (4-row window); the image is not drawn |
| A track with no cover, an empty queue, a restored stopped player | Area blank, window tall |
| The cache directory is unwritable, or `max_cover_arts = 0` | The player cannot cache (0010: MPRIS gets the `https://` URL); the TUI's area stays blank |
| The player's download fails or is slow | Blank until the file appears; the TUI looks once a second |
| Network down | Blank, one `warn` per cover, retried when it becomes current again |
| Disconnected from the daemon (0005) | The last cover stays while the last state is shown, as the text does |
| Track change while the previous cover still encodes | The old result is dropped; the new one is asked for |
| Two attached TUIs | Each reads the same cached file |
| A daemon with `TIDAL_PLAYER_CACHE_DIR` set only in its unit | The TUI looks in another directory and shows no covers; documented (set it for both, or neither) |
| `max_cover_arts` lowered | As 0010: applied after the next write and at the player's next start |

## Test plan

Each test is named after its criterion. Red is a failing assertion against stub types and functions with stub bodies (`cover_layout` returns today's layout, `protocol` returns none, the loader never yields an image, the config ignores the keys), no `todo!()` and no compile errors, as in 0001–0014.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/app/src/ui.rs` :: `ac1_cover_layout` (table) | rects per size, rows, cell size | stub returns the 4-row layout with no cover |
| AC2 | `crates/app/src/ui.rs` :: `ac2_list_height_with_cover` (table) | list rows with and without the cover | stub ignores the cover |
| AC3 | `crates/app/src/cover.rs` :: `ac3_protocol` (table) | protocol, query or not | stub returns none and always queries |
| AC4 | `crates/app/src/cover.rs` :: `ac4_slot` (table of event sequences) | requests, kept and dropped results, blank, no retry | stub keeps every result |
| AC5 | `crates/app/src/ui.rs` :: `ac5_cover_window` (snapshot per row) | window height, text column, progress row | stub draws today's window |
| AC6 | the existing snapshots (unchanged); `crates/app/src/ui.rs` :: `ac17_no_panic_any_size` (new rows) | nothing changes when inactive; no panic | passes in red (guard); the new rows fail only if the stub panics |
| AC7 | `crates/app/src/ui.rs` :: `ac7_cover_drawn_in_rect` | fake protocol's rect, size mismatch, none | stub never draws |
| AC8 | `crates/app/src/cover.rs` :: `ac8_cached`, `ac8_appears_later`, `ac8_read_only`, `ac8_decode_failure`, `ac8_newest_wins` | images, no request, cache untouched, logs | stub loader never yields; `ac8_read_only` passes in red (guard) |
| AC11 | `crates/app/tests/mpris.rs` :: `ac11_cache_without_mpris` (rows: `mpris = false`, no bus, `max_cover_arts = 0`) | file written or not, requests | stub starts the worker only with MPRIS |
| AC9 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows); `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` | values, errors, example | stub config ignores the keys |
| AC10 | `crates/app/tests/docs.rs` :: `ac10_cover_documented` + zh-TW copies reviewed at acceptance | docs mention the keys and tmux | stub docs lack them |

Checked by hand at acceptance on the user's machine (results in the PR description), because they need real terminals and no test can see what a terminal drew: the cover shows and changes with the track in kitty (kitty protocol), foot or WezTerm (sixel / iTerm2), and under tmux with `allow-passthrough on`; GNOME Terminal (or another terminal without a protocol) shows today's 4-row window; `cover_img_protocol = "halfblocks"` draws blocks; resizing redraws it cleanly with no leftover pixels; opening and closing popups and the keys help leaves no garbage over the cover; an attached TUI shows the daemon's track's cover; `cover_img = false` gives today's screen.

## Crate placement

- `tidal-player` (binary): a new `cover.rs` (the protocol choice and `Slot` (pure), the loader thread: read-only cache lookup, decode, encode, behind a seam that tests fake; it never downloads and has no HTTP client); `ui.rs` (`cover_layout`, the taller playback window, `render` taking the ready image, `list_height` with the cover's rows); `main.rs` (the query at start, the loader wired to the state and the redraw); `config.rs` (the three keys). 0010's `mpris/covers.rs` moves out of `mpris` (`crate::covers`): the player starts its `CoverWorker` whenever `max_cover_arts > 0`, independent of MPRIS, and the MPRIS adapter, when it runs, takes its results; the TUI uses only its read-only path and directory functions. Its tests move with it unchanged
- New workspace dependencies: `ratatui-image` 11 (`default-features = false`, `features = ["crossterm"]`: no `chafa`, so no new system library) and `image` 0.25 (`default-features = false`, `features = ["jpeg"]`), for the binary only
- `tidal-player-core`: no change (`cover` and `cover_url` exist). `xtask layering`: no change (images stay in the binary)

## Facts vs. assumptions

Verified (2026-10-12, from code and crates.io):

- The playback window is 4 rows (`PLAYBACK_ROWS`), the page needs 6 inner rows (`MIN_ROWS_WITH_QUEUE`), the title side's narrowest is 20 columns (`MIN_LEFT`), all in `crates/app/src/ui.rs`; `areas` lays out the frame and `list_height` uses it; `main.rs` sends `Resize` from `list_height`
- Popups, the actions menu and the key hints are drawn inside the page area (`pages::render_popup`, `render_hint` take `areas.page`)
- The cover cache: `crates/app/src/mpris/covers.rs` (`CoverCache::{lookup, download, fetch}`, `CoverWorker`, `process_cache_dir`, `evict`), started by the MPRIS adapter (`mpris/hub.rs`, `CoverWorker::spawn`), so it downloads only while MPRIS is on and available (0010)
- `ratatui-image` 11.1.0 depends on `ratatui ^0.30.1` (ours: 0.30.2) and `image ^0.25.6`; its default features pull `chafa-dyn` (pkg-config); it has `Picker::from_query_stdio` (to be called after entering the alternate screen and before reading events), protocols sixel, kitty, iTerm2 and halfblocks, and tmux passthrough (`is_tmux` from the environment). It blacklists kitty and sixel under WezTerm and Konsole and uses iTerm2 there. 12.0 is a release candidate only
- (from its source, 11.1.0) `from_query_stdio` waits at most 2 s (`STDIN_READ_TIMEOUT_MILLIS`), the wait restarted by each "busy" step of its query thread; a terminal that answers does so at once (the query ends with a status report every terminal answers), so only one that does not answer pays it. tmux is detected from `TERM`/`TERM_PROGRAM`, not `$TMUX`, and the crate runs `tmux set -p allow-passthrough on` for its pane. A terminal that answers without a cell size gets the crate's default halfblocks picker

Assumptions, checked at acceptance:

- The terminals listed under "Context" answer the query as `ratatui-image`'s compatibility matrix says
- Redrawing the cells under a sixel image (resize, a popup) clears it in foot, WezTerm and xterm; kitty's images go with their cells
- spotify-player (from memory, re-check): `cover_img_length`, `cover_img_width`, `cover_img_scale` behind its `image` feature, drawn with `viuer`

## Decisions

Answered by the user, 2026-10-12 (approved with the spec):

1. **Height**: the playback window grows while the cover is active (`cover_img_rows`, default 6), rather than fitting an 8 × 4 cover into today's 4 rows
2. **No protocol**: no cover (today's window), no automatic text-block fallback; `cover_img_protocol = "halfblocks"` forces one
3. **Where the image comes from**: the TUI reads the shared cache; the protocol is unchanged
4. **The TUI never downloads** (answered 2026-10-12, replacing the proposal that it download what is missing): it only checks whether the player (daemon or standalone, MPRIS's worker) has cached the file. So the player caches the current cover whenever `max_cover_arts > 0`, MPRIS or not (a change to 0010)
5. **Default**: on (`cover_img = true`)
6. **Stable layout** (answered 2026-10-12: yes): the window is tall whenever the cover is active, even with nothing playing or a track without a cover, so pages do not change height between tracks
7. **Progress bar on the last row** of the taller window, the text on the first three (approved with the spec)

## Out of scope

- Covers anywhere else (pages, the queue, a full-screen cover view)
- Prefetching the next entry's cover; other image sizes than `640x640`
- The player telling clients where its cached cover is (decision 3)
- Text-block (`chafa`/ASCII) renderers beyond `halfblocks`
- Choosing the cover's position (right side) or a width independent of its height

## Implementation notes

- **Slice A (AC11)**: the cover worker is the MPRIS adapter's (`mpris::hub::Adapter`), which is a plain in-process hub client (attach, subscribe) that follows the current entry and drives the one `CoverWorker`. `mpris::start` now builds it whenever `max_cover_arts > 0` or MPRIS is on; the bus is connected only when MPRIS is on, and `set_bus` is simply never called when it is off or unavailable, so no second downloader exists. `cache.start()` (clean-up and limit) runs once per player start, before the bus is tried. `Mpris::name()` is `None` for a cache-only follower
- 0010's `ac11_no_bus` and `ac13_mpris_off` asserted "nothing was cached without MPRIS", which this spec reverses; those two lines were removed (`ac11_cache_without_mpris` covers the new behaviour)
- **Slice B (AC1–AC10)**:
  - **Threads**: the UI thread owns `cover::Cover` (a `Slot` and a `Loader`) and calls `update(current cover, cover area size, now)` once per frame (0004's 100 ms tick), never blocking: it takes the loader's results with `try_recv`, feeds the slot and sends the slot's newest request. The loader is one `std::thread` (`cover-loader`) that takes the newest waiting request, reads `<cache dir>/covers/<cover>.jpg` (`covers::cached_file`, which refuses anything that is not a cover ID), decodes it (`image`, JPEG only) and encodes it (`Picker::new_protocol`, `Resize::Fit`) for the area's size, then sends back the `ratatui_image::protocol::Protocol` (it is `Send`). It has no HTTP client and only `std::fs::read`s; the one-second look is the slot's `look_at`, driven by the frame's clock. A failure is one `tracing::warn!` per look, and the slot does not look again until the cover becomes current again. `render_with_cover` gets a `CoverFrame` (geometry, and the ready image with the size it was encoded for) and draws the image last, in the cover rect, only when that size is the rect's and the keys help does not cover it; `render` is `render_with_cover(…, None)`, so every existing snapshot is drawn by the same code with the cover inactive
  - **Where the query runs**: `main.rs::start_cover`, after raw mode, the alternate screen and bracketed paste, before the `Terminal` is built and before the first `event::poll`; only when `cover_img` is on (`cover::protocol` calls the query closure only then). The queried `Picker` is kept for encoding (its cell size and tmux flag); with no answer and a forced protocol, `Picker::halfblocks()` (10 × 20 px cell, so 2 columns per row) with that protocol
  - **`cover_img_*` plumbing**: as `key_hints` (`AppConfig` → `PlayerSettings` in `play.rs` → `cover::Settings`). The daemon parses and ignores them; like every setting, the daemon reads the variables after the session check, so an invalid `TIDAL_PLAYER_COVER_IMG*` stops a daemon only once it is logged in. The TUI also reads its own `max_cover_arts`: `0` looks for nothing (old files could remain in the cache, the player does not clean it with the cache off)
  - **Spec numbers corrected**: AC1's first example is a 78 × 22 inner (an 80 × 24 terminal), text width 65 (an 80-wide inner gives 67, also a test row). AC5's 40 × 12 is *not* too narrow by the stated limits (38 ≥ 12 + 1 + 20, 10 ≥ 6 + 2): its snapshot shows the cover active with a 25-column text side, and the "too narrow" case is checked at 34 × 12 (32 < 33) and "too short" at 80 × 9, both equal to today's drawing
  - **The keys help** (at least 40 × 10) can reach over the playback window when the page is short even though there is a page area; the rule "anything drawn over the cover area: the cover is not drawn" handles it (`pages::help_rect`, AC7's test)
  - **`ratatui-image` 11.1 facts** (from its source): `from_query_stdio`'s timeout is 2 s (`STDIN_READ_TIMEOUT_MILLIS`, restarted by each "busy" step), not "well under a second"; real terminals answer at once (the query ends with a DSR every terminal answers), so only a terminal that never answers waits it out. A terminal that answers without a cell size (xterm with window ops disallowed) gets the crate's default halfblocks picker, so `auto` gives none there and `cover_img_protocol = "sixel"` is the fix; with no answer it falls back to the window-size ioctl for the cell and to environment hints (iTerm2/WezTerm) for the protocol. Konsole and WezTerm have kitty and sixel blacklisted (WezTerm then uses iTerm2 from `TERM_PROGRAM`; Konsole gets halfblocks, so none under `auto`). tmux is detected from `TERM` (`tmux…`) or `TERM_PROGRAM=tmux`, not `$TMUX`, and the crate itself runs `tmux set -p allow-passthrough on` for its pane. These are now in "Facts vs. assumptions", Behaviour (Protocol), Edge cases and the docs; AC1 and AC5 were corrected in place. `default-features = false` keeps `from_query_stdio` (it needs only `rustix` on Unix, always on) and drops `chafa`
  - **Known limit**: if a terminal never answers, the crate's query thread can stay blocked reading stdin after the timeout and swallow the first key pressed (read from its source, not reproduced); to check by hand on the Linux console at acceptance
