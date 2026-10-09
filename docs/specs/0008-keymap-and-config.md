# 0008 — Keymap, config files and the keys help

- **Status**: implemented (2026-10-08; the manual checks under "Test plan" are run on the user's machine)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0003–0007 (implemented: every setting and key this spec moves into files)
- **User docs**: a new [`docs/config.md`](../config.md) (the two files, every setting, every command, the key syntax); [`docs/tui.md`](../tui.md) "Keys" names each key's command and gains the keys help; [`docs/playback.md`](../playback.md) "Settings" and [`docs/daemon.md`](../daemon.md) point at `app.toml` (AC17)

## Context

Settings come from the environment only (0003–0007 "Settings", eleven variables), so the daemon's are edited as `Environment=` lines in its systemd unit (0005), and keys are a hard-coded `match` spread over `key_press`, `browse_key`, `popup_key`, `whole_list_key` and `search::window_key` in `tidal_player_core::ui`. Every spec since 0003 has promised this one: **`app.toml`** for the settings (the variables keep working), **`keymap.toml`** for the keys, in spotify-player's format so a `[[keymaps]]` block can be copied across, and (0006, the dropped 0012 draft, the user 2026-10-08) the **keys help** popup, `?`, generated from the same bindings so it cannot drift.

The keys help follows the user's request (2026-10-08, kept from the 0012 draft): lazygit's version of spotify-player's help: the keys of **where you are first**, then the global ones, filterable with `/`, and `Enter` runs the highlighted key.

### What tidalt did

Read from tidalt's `internal/ui/appconfig.go`, `keymap.go`, `help.go` (`ee9c24e`, `f66b3f0`, `ee0070e`):

- `~/.config/tidalt/app.toml` (layout only: a preset and a few switches) and `keymap.toml` mirroring spotify-player's `[[keymaps]]` (`command`, `key_sequence`), plus a `preset` key choosing a whole tidalt or spotify-player key set
- **Unknown settings and commands were errors** (`unknown setting "…"`, `keymaps[3]: unknown command "…"`), but a broken file **did not stop the program**: it fell back to the preset's keys and showed the error in the UI. A daemon has no UI, so the error went to its log only and the user saw the defaults with no reason
- No `[[actions]]`, no parameters (`VolumeChange { offset }`), and the playback settings (quality, device) stayed in the environment, so two places configured one program
- The help popup was generated from the keymap but showed every key everywhere, including ones that did nothing in the current view (0012 draft)
- The preset switch made `app.toml` change what `keymap.toml` meant (`ee0070e` fixed a key set that did not follow the layout preset): two files depending on each other

### What could go wrong here, and the criterion that covers it

1. **A typo silently ignored** (`volum_step = 10` does nothing; tidalt caught this) → unknown settings, commands, actions and keys are errors naming the file, the entry and the problem (AC2, AC10)
2. **A broken file ignored by a headless daemon** (tidalt: logged, defaults used) → every mode refuses to start on a broken file, exit 2, before the terminal or the socket is touched (decision 2) (AC12)
3. **Help or docs that lie** (a key listed that does nothing, or not listed) → dispatch, help and docs all read the one keymap; a test runs every binding through dispatch, another checks every command is documented (AC3, AC17)
4. **Rebinding a key breaks typing**: `q` rebound must still type `q` in a prompt or the search input → text inputs take printable keys before the keymap (AC6)
5. **A sequence that can never fire**: binding `g` alone makes `g g`, `g l`, … unreachable in spotify-player (the first exact match wins) → a sequence that is a prefix of another is an error naming both (decision 4) (AC2)
6. **Two sources of truth for one setting**: environment, flag and file → one precedence, flag > environment > file > default, tested per setting (AC11)
7. **Behaviour change by refactor**: moving the keys into a table must not change what any key does → 0004–0007's key tests stay unchanged and green (AC3)

## Behaviour

### Where the files are

The **config directory** is `--config-folder <DIR>` (`-c`, spotify-player's flag, on every command), else `$TIDAL_PLAYER_CONFIG_DIR`, else `$XDG_CONFIG_HOME/tidal-player`, else `~/.config/tidal-player` (0001 "Naming"). It holds `app.toml` and `keymap.toml`; either may be missing (defaults), and so may the directory. Nothing is ever written there: no file is created, no default is dumped. An empty variable counts as unset.

Files are read **once, at start**, by each process for its own part: the player (standalone TUI, daemon, `play`) for the player settings, the TUI (standalone or attached) for the client settings and the keymap. A daemon picks up a changed file when restarted (`systemctl --user restart tidal-player`); there is no live reload.

### `app.toml`

Flat keys, one per existing setting, plus spotify-player's library layout. Every key is optional.

| Key | Accepted | Default | Replaces (still read, and beats the file) | Read by |
|---|---|---|---|---|
| `quality` | `"hi-res"`, `"lossless"`, `"high"` | `"hi-res"` | `--quality`, `TIDAL_PLAYER_QUALITY` | player |
| `output_device` | any ALSA PCM name | `"default"` | `--device`, `TIDAL_PLAYER_DEVICE` | player |
| `volume_step` | integer 1–25 (%) | `5` | `TIDAL_PLAYER_VOLUME_STEP` | TUI |
| `seek_duration_secs` (spotify-player's name) | integer 1–600 | `5` | `TIDAL_PLAYER_SEEK_STEP` | TUI |
| `previous_restart_secs` | integer 0–60 | `3` | `TIDAL_PLAYER_PREVIOUS_RESTART` | player |
| `autoplay` | `true`, `false` | `false` | `--autoplay`, `TIDAL_PLAYER_AUTOPLAY` | player |
| `release_paused_secs` | integer 0–3600, or `"never"` | `10` | `TIDAL_PLAYER_RELEASE_PAUSED` | player |
| `page_size` | integer 1–10 000 | `100` | `TIDAL_PLAYER_PAGE_SIZE` | player, TUI |
| `search_page_size` | integer 1–1000 | `20` | `TIDAL_PLAYER_SEARCH_PAGE_SIZE` | player, TUI |
| `hide_versions` | array of strings (`[]` hides nothing) | 0006's sixteen words | `TIDAL_PLAYER_HIDE_VERSIONS` | player |
| `[layout] library = { playlist_percent, album_percent }` (spotify-player's) | integers 1–98 each, sum at most 99; *Artists* takes the rest | `40`, `40` | (was fixed, 0006) | TUI |

```toml
# ~/.config/tidal-player/app.toml
quality = "lossless"
output_device = "hw:1,0"
seek_duration_secs = 10
release_paused_secs = "never"
hide_versions = ["instrumental", "karaoke"]

[layout]
library = { playlist_percent = 30, album_percent = 50 }
```

- **Precedence**: flag > environment > `app.toml` > default, per setting (decision 5). `hide_versions` keeps 0006's matching (lower-cased, stripped of spaces, hyphens, underscores, dots and `+`)
- **Errors** (exit 2, nothing started; decision 2):
  - TOML syntax: `<path>: line 3, column 9: <parser message>`
  - Unknown key, anywhere: `<path>: unknown setting "volum_step"`
  - Wrong type or range: `<path>: invalid volume_step: expected an integer from 1 to 25, got 0` (strings as `got "x"`); the messages read as the variables' (0004 "Settings") with the key in place of the variable
  - A file that exists but cannot be read (permissions, a directory): `<path>: <io error>`
- Every process validates the **whole** file, whichever part it reads: a typo in a TUI key fails the daemon too, so one restart shows it

### `keymap.toml`

spotify-player's format: entries that **add to or replace** the default bindings.

```toml
# ~/.config/tidal-player/keymap.toml
[[keymaps]]
command = "NextTrack"
key_sequence = "g n"          # g n now also skips; n still does

[[keymaps]]
command = "None"
key_sequence = "q"            # q no longer quits (C-c still does)

[[keymaps]]
command = { SeekForward = { duration = 30 } }
key_sequence = "L"

[[keymaps]]
command = { VolumeChange = { offset = 1 } }
key_sequence = "="

[[actions]]
action = "GoToAlbum"
key_sequence = "g B"
target = "PlayingTrack"       # or "SelectedItem" (the default)
```

- **Key sequences** (spotify-player's syntax): keys separated by single spaces. A key is a single character (`a`, `A` for Shift-a, `>`, `?`), or a name: `enter`, `space`, `tab`, `backtab`, `backspace`, `esc`, `left`, `right`, `up`, `down`, `insert`, `delete`, `home`, `end`, `page_up`, `page_down`, `f1`–`f12`; optionally prefixed by `C-` (Control) or `M-` (Alt): `C-s`, `M-enter`, `C-space`. `C-S` is `C-s` (terminals report Control-letters without case). Anything else is an error naming the key: `<path>: keymaps[2]: unknown key "ctrl+s" in "ctrl+s"`
- **`[[keymaps]]`**: `key_sequence` and `command`; `command` is a name, `"None"` (removes whatever that sequence is bound to), or a one-entry inline table for the commands with parameters. An entry replaces the default binding of the **same** sequence only; a command keeps its other sequences (above, `n` still skips). Later entries beat earlier ones
- **`[[actions]]`**: `key_sequence`, `action`, optional `target` (`"SelectedItem"`, default, or `"PlayingTrack"`): runs that entry of the actions popup (0006 "Actions") directly on the selected row or the playing track, without opening the popup. On a row where the popup would not list it (*Go to album* on an artist, *Play next* on the playing track), it does nothing
- **Errors** (exit 2, nothing started): TOML syntax and unknown fields as for `app.toml`; an unknown command or action (`<path>: keymaps[4]: unknown command "NxtTrack"`); a bad parameter (`<path>: keymaps[5]: VolumeChange offset must be from -25 to 25 and not 0, got 40`); an empty sequence; a **prefix conflict** after merging with the defaults: `<path>: "g" is bound to LibraryPage and is the start of "g g" (SelectFirstOrScrollToTop), "g l", …; unbind those with command = "None" first` (decision 4)
- **spotify-player commands this player does not have** (`PlayRandom`, `SwitchTheme`, `SwitchDevice`, `BrowseUserPlaylists`, `LyricsPage`, `SortTrackByTitle`, … the list is in `docs/config.md`): the entry is skipped and the TUI's message row says, once at start, `keymap.toml: 3 spotify-player commands not supported here: PlayRandom, LyricsPage, SwitchTheme` (decision 3). So a spotify-player `keymap.toml` can be copied as is

### Commands and their default keys

The defaults are exactly the keys of 0004–0007, plus spotify-player's defaults for `?`/`C-h`, `C-n`/`C-p`, `end` and `/` where they had none here. Names are spotify-player's where the behaviour is the same, and new names otherwise (marked *).

| Command | Default keys | Acts in | Help text |
|---|---|---|---|
| `ResumePause` | `space` | everywhere | play / pause |
| `NextTrack` | `n` | everywhere | next track |
| `PreviousTrack` | `p` | everywhere | previous track |
| `SeekForward { duration? }` | `>` | everywhere | seek forward (seconds; default `seek_duration_secs`) |
| `SeekBackward { duration? }` | `<` | everywhere | seek backward |
| `SeekStart` | `^` | everywhere | back to the start of the track |
| `Shuffle` | `C-s` | everywhere | shuffle on / off |
| `Repeat` | `C-r` | everywhere | repeat: off → queue → track |
| `ToggleAutoplay`* | `A` | everywhere | autoplay on / off |
| `VolumeUp`*, `VolumeDown`* | `+`, `-` | everywhere | volume up / down by `volume_step` |
| `VolumeChange { offset }` | — | everywhere | volume by `offset` % (−25…25, not 0) |
| `Mute` | `_` | everywhere | mute / unmute |
| `AddToQueuePrompt`*, `PlayNextPrompt`* | `o`, `O` | everywhere | add a link or ID to the end of the queue / to play next |
| `SelectNextOrScrollDown` | `j`, `down`, `C-n` | lists, popups | move down |
| `SelectPreviousOrScrollUp` | `k`, `up`, `C-p` | lists, popups | move up |
| `PageSelectNextOrScrollDown` | `C-f`, `page_down` | lists, popups | move down a window |
| `PageSelectPreviousOrScrollUp` | `C-b`, `page_up` | lists, popups | move up a window |
| `SelectFirstOrScrollToTop` | `g g` | lists, popups | move to the top |
| `SelectLastOrScrollToBottom` | `G`, `end` | lists, popups | move to the last loaded row |
| `ChooseSelected` | `enter` | lists, popups | play the track with its list / open / run |
| `AddSelectedItemToQueue` | `Z`, `C-z` | lists | add to the end of the queue |
| `RemoveFromQueue`* | `d` | the queue | remove from the queue |
| `ShowActionsOnSelectedItem` | `g a`, `C-space` | lists | actions on the selected row |
| `ShowActionsOnCurrentTrack` | `a` | everywhere | actions on the playing track |
| `FocusNextWindow`, `FocusPreviousWindow` | `tab`, `backtab` | pages | next / previous pane |
| `NextTab`*, `PreviousTab`* | `]`, `[` | panes with tabs | next / previous tab |
| `RoleFilter`* | `f` | the artist's *All tracks* | the role filter |
| `Queue` | `z` | everywhere | the queue page |
| `LibraryPage` | `g l` | everywhere | the library |
| `LikedTrackPage` | `g y` | everywhere | favorite tracks |
| `SearchPage` | `g s` | everywhere | the search page (on one: its input) |
| `Search` | `/` | the search page | back to the search input (spotify-player's in-page search; see "Out of scope") |
| `PreviousPage` | `backspace`, `C-q` | everywhere | back |
| `ClosePopup` | `esc` | popups, prompts, loads | close / cancel |
| `OpenCommandHelp` | `?`, `C-h` | everywhere | this help |
| `Quit` | `q`, `C-c` | everywhere | quit (an attached TUI detaches) |

Actions for `[[actions]]` (0006's popup entries; spotify-player's names, ours marked *): `GoToAlbum`, `GoToArtist` (the first artist; the popup lists each), `AddToQueue`, `PlayNext`*, `AddToLiked`, `DeleteFromLiked`, `AddToPlaylist` (opens the playlist picker), `DeleteFromPlaylist` (own playlist pages), `RemoveFromQueue`* (queue entries), `DeletePlaylist`* (own playlists, still asks `y/n`). spotify-player actions without a counterpart (`GoToRadio`, `CopyLink`, `ToggleLiked`, `Follow`, …) are skipped like unsupported commands.

### Key sequences and where keys act

- **Sequences** (spotify-player's rule, as 0006 has it for `g`): pressed keys collect; when what has been collected is no binding's start, collecting restarts with the last key alone (`g x` does what `x` does); when it equals a binding, that binding runs and collecting restarts. There is no timeout. Sequences may be any length (`s l a`)
- **Where a key acts**: a binding acts only where its command does (the "Acts in" column); elsewhere it does nothing (as `d` off the queue today). Popups keep 0006's rule, "only its own keys act": in a popup the list commands, `ChooseSelected`, `ClosePopup` and `OpenCommandHelp` act, through the keymap
- **Fixed keys** (not in the keymap, decision 6): text inputs (the `o`/`O` prompt, the search input, the playlist-name prompt) take printable characters and paste before the keymap, with 0004/0006/0007's editing keys (`Backspace`, `C-u`, `Enter`, `Esc`, `Tab`/`BackTab` in the search input; `C-c` quits and `C-q` goes back from the search input); the role filter's `Space` and the confirmations' `y`/`n`

### The keys help (`?`)

`OpenCommandHelp` (`?`, `C-h`) opens it anywhere except in a text input. Centred, 80 % of the page area (at least 40 × 10, clipped to the terminal), titled `Keys`:

```
┌Keys────────────────────────────────────────────────────┐
│ Library · Playlists                                    │
│   enter            open the playlist                   │
│   Z  C-z           add to the end of the queue         │
│ Lists                                                  │
│   j  down  C-n     move down                           │
│   k  up  C-p       move up                             │
│   …                                                    │
│ Pages                                                  │
│   z                the queue page                      │
│   g l              the library                         │
│ Playback                                               │
│   space            play / pause                        │
│ App                                                    │
│   ?  C-h           this help                           │
│   q  C-c           quit                                │
│ / filter · enter run · esc close                       │
└────────────────────────────────────────────────────────┘
```

- **Keys shown** are the effective ones, after `keymap.toml`, written in the file's syntax (`C-s`, `backtab`, `space`), so what the help shows can be pasted into the file. `[[actions]]` bindings are listed under `Actions`
- **Sections**: the open popup's (when opened from one), then the focused window's (`Queue`, `Library · Playlists`, `Album`, `Artist · All tracks`, `Search · Tracks`, …), then `Lists` (on list windows), `Pages`, `Playback`, `Actions`, `App`. A section lists the bindings of its context; one that would do nothing right now (playback keys on an empty queue, while disconnected) is drawn dim, not hidden. A command with no key is not listed
- **Moving**: the list commands, through the keymap, over binding rows (headings skipped)
- **Filter** (lazygit's `/`, a fixed key here): typed characters keep the rows whose keys, command name or help text contain them, ignoring case; empty sections disappear; `Backspace` deletes; `Enter` ends typing and keeps the filter; `Esc` clears it. Nothing matches: `No keys match "xyz"`
- **Running** (lazygit's `Enter`): `Enter` on a row closes the help and runs the binding, exactly as its keys would in the view the help was opened from
- **Closing**: `Esc` (no filter), `?`, `q` close it with no effect. While it is open no other key acts

### The library layout

`[layout] library` sets the library page's window widths (0006: 40 / 40 / 20): *Playlists* `playlist_percent`, *Albums* `album_percent`, *Artists* the rest, when drawn side by side (60 columns or more inside; below that, one window, as now).

## Acceptance criteria

Keymap (`tidal_player_core::ui::keymap`, pure; no TOML here: it takes the entries already deserialised):

- **AC1** — Key syntax (table): every key name, single characters (`A`, `>`, `?`, `-`), `C-`/`M-` with characters and names, `C-S` = `C-s`, multi-key sequences parse; `""`, `"ctrl+s"`, `"C-"`, `"X-a"`, two spaces, a trailing space are errors naming the key. Every parsed sequence prints back as written (the label the help and docs use)
- **AC2** — Building the keymap from the defaults and the entries (table): the defaults equal the "Commands and their default keys" table; an entry adds a sequence; one with a default sequence replaces it; `None` removes it; a later entry beats an earlier one; parameters (`VolumeChange` in range, out of range, 0; `SeekForward` with and without `duration`, `duration` 0 and 601 refused); `[[actions]]` with both targets and the default; an unknown command or action → an error with the entry's index; a spotify-player command or action without a counterpart → skipped and named in the result's notice list; a prefix conflict (`g` bound; `s` and `s l a`) → an error naming both sequences; removing the conflicting defaults with `None` makes it valid; unbinding every `Quit` key → `Quit has no key left`
- **AC3** — Dispatch goes through the keymap: `key_press`, the popup handlers and the search window keys look keys up in `State`'s keymap. 0004–0007's key tests run **unchanged** with the default keymap and stay green; a table test runs every default binding in a state where its command acts and checks the effects equal running the command directly
- **AC4** — Rebinding (table, model): `NextTrack` on `g n` → `g n` sends `Next` and `n` still does; `None` on `q` → `q` does nothing, `C-c` quits; `ResumePause` moved to `M-p` → `M-p` toggles, `space` does nothing; `s l a` bound → `s` waits, `s l a` runs, `s x` does what `x` does; `VolumeChange { offset = 1 }` sends `ChangeVolume(1)`; `SeekForward { duration = 30 }` sends `SeekBy(30000)`
- **AC5** — `[[actions]]` (table): each action with `SelectedItem` on each row kind emits what choosing that entry in the actions popup emits; with `PlayingTrack`, what the popup opened with `a` emits; on a row where the popup would not list it, nothing; with nothing playing and `PlayingTrack`, nothing
- **AC6** — Text inputs before the keymap (table): with `q`→`NextTrack`, `j`→`Quit`, `?` bound, typing `qj?` in the `o` prompt, the search input and the playlist-name prompt types `qj?` and emits nothing; their editing keys and the role filter's `Space` and the confirmations' `y`/`n` work whatever the keymap says

Keys help (`tidal_player_core::ui::help`, pure):

- **AC7** — Contents (`help(&State) -> Vec<HelpSection>`, table over contexts: queue page; library with each window focused; favorite tracks; album; own and followed playlist; artist with each of its four windows; search with the input and with each window; each popup open): section titles in order, each section exactly its context's bindings with their effective keys (one case with a custom keymap: the moved key shown, the removed one gone, the `[[actions]]` section present), dim rows where the command would do nothing (empty queue, disconnected)
- **AC8** — Keys (table): `?` and `C-h` open it from every page and popup, type into every text input; list commands move over binding rows only, clamped; `/` filters on keys, command names and help text ignoring case, `Backspace` edits, `Enter` keeps, `Esc` clears and a second `Esc` closes; `Enter` on a row closes the help and emits exactly what pressing that binding's keys in the original view emits (`space` → `Send(TogglePause)`, `g l` → the library pushed and fetched, an `[[actions]]` row → its action); `?`/`q` close with no effect; no other key acts

Config files (`tidal-player::config`, pure functions over strings and a fake environment):

- **AC9** — Config directory (table): flag; `TIDAL_PLAYER_CONFIG_DIR`; empty variable ignored; `XDG_CONFIG_HOME`; home only; flag beats the variable
- **AC10** — `app.toml` (table): every key at its bounds and one past each, wrong types, `"never"`, `hide_versions = []`, the layout bounds (1, 98, a sum of 99 and of 100); an unknown top-level key and an unknown key under `[layout]`; a syntax error naming line and column; an empty file = defaults. Each error message as under "Errors"
- **AC11** — Precedence (table, per setting): default; file; environment over file; flag over both (`--quality`, `--device`, `--autoplay`); an empty variable falls through to the file; an invalid variable is an error even when the file is valid. `resolve_player_config` keeps 0003–0007's tests green unchanged
- **AC12** — Loading (`assert_cmd`, a temp config dir): a missing directory or file runs with defaults; a broken `app.toml` or `keymap.toml` makes `tidal-player daemon`, `tidal-player play …` and `tidal-player` exit 2 with the message on stderr, before binding the socket and before raw mode (the test's stderr is clean text, no escape sequences); `keymap.toml` with an unsupported spotify-player command starts, and the TUI's first frame shows the notice (model test on the notice list → `State::message`)

TUI (`tidal-player`):

- **AC13** — Key decoding (`input.rs`, table): `M-` keys, `left`, `right`, `home`, `end`, `insert`, `delete`, `f1`–`f12` decode (today Alt and F-keys are dropped); everything 0004–0007 decoded still decodes the same
- **AC14** — Rendering (`insta`, 80×24, reviewed by eye, + `contains`): the help on the queue page, on the library with *Albums* focused, filtered by `fav`, opened from the actions popup, with a custom keymap; 40×12 keeps the key column and cuts help text with `…`. The library at `playlist_percent = 30, album_percent = 50`. No panic from 0×0 to 120×40 (0004 AC17's test, new rows)
- **AC15** — Wiring: the TUI's `State` gets its keymap, steps, page sizes and library layout from the files (`tui_state`, table: defaults, a file, a variable over a file); the daemon's `PlayerSettings` come from `app.toml` (the test player of 0005 AC14's harness with `quality`, `release_paused_secs` and `page_size` from a file in a temp config dir)
- **AC16** — An attached TUI with a different `keymap.toml` than the daemon's uses its own: keys are the client's alone (two clients over one test player, `n` rebound in one)

Docs:

- **AC17** — `docs/config.md` documents both files, every key, every command and action with default keys, the key syntax, the errors, and the unsupported spotify-player names; `docs/tui.md` "Keys" names each key's command and documents the keys help; `docs/playback.md` "Settings" and `docs/daemon.md` point to `app.toml` (the unit's `Environment=` lines still work). A test checks that every command and action in the keymap appears in `docs/config.md` with its default keys. `CLAUDE.md` "Status" names this spec; this spec links to them

Default files (added 2026-10-08 at the user's request, "gen a default keymap and app toml"):

- **AC18** — `examples/app.toml` sets every `app.toml` key to its default, and resolving it with no flags or variables gives exactly the defaults; `examples/keymap.toml` has one `[[keymaps]]` entry per default binding (the "Commands and their default keys" table, same sequences and commands, nothing else) and builds to the default keymap. Copying either into the config directory changes nothing, so they are starting points to edit. `docs/config.md` and `README.md` link them

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| No config directory | Defaults; nothing created |
| `app.toml` is a directory, or unreadable | Exit 2, `<path>: <io error>` |
| A file with a UTF-8 BOM or CRLF line ends | Read as TOML allows (the `toml` crate accepts both); a test row each |
| Same sequence twice in `keymap.toml` | The later entry wins; no error |
| A command bound to no key | Not reachable, not in the help; no error |
| Every `Quit` key unbound | Refused: `<path>: Quit has no key left` (exit 2), so the TUI can always be left |
| `?` bound to something else, `OpenCommandHelp` unbound | Allowed if `Quit` still has a key; help unreachable |
| A key the terminal cannot send (`C-enter` in many terminals) | Accepted; never fires. `docs/config.md` notes which are unreliable |
| `C-i`, `C-m`, `C-[` | Terminals send `tab`, `enter`, `esc`; documented, not special-cased |
| Help in a terminal smaller than 40 × 10 | Clipped to what there is; no panic (AC14) |
| Attached TUI and daemon read different config dirs | Each uses its own: settings are per process as listed under "Where the files are" |
| `--config-folder` pointing at a missing directory | Defaults (as a missing default directory); not an error |
| Environment and file disagree | The environment wins, silently (it is the explicit, per-run choice) |

## Test plan

Each test is named after its criterion (`ac4_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0007.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/keymap/tests.rs` :: `ac1_key_syntax` (table) | parses, errors, labels round trip | stub parser accepts single characters only |
| AC2 | `crates/core/src/ui/keymap/tests.rs` :: `ac2_build_keymap` (table) | defaults, merge, errors, notices | stub `build` returns the defaults and ignores entries |
| AC3 | the existing key tests in `crates/core/src/ui/mod.rs`, `browse/tests.rs`, `search/tests.rs` (unchanged) + `crates/core/src/ui/keymap/tests.rs` :: `ac3_every_binding_dispatches` | old behaviour; binding = command | stub `run_command` does nothing, so the coverage test fails (the old tests stay green: they are the refactor's safety net, not its red) |
| AC4 | `crates/core/src/ui/keymap/tests.rs` :: `ac4_rebinding` (table) | effects per key with a custom keymap | stub dispatch ignores the state's keymap and uses the defaults |
| AC5 | `crates/core/src/ui/keymap/tests.rs` :: `ac5_action_bindings` (table) | same effects as the popup | stub action bindings do nothing |
| AC6 | `crates/core/src/ui/keymap/tests.rs` :: `ac6_text_inputs_first` (table) | typed text, no effects | stub looks the keymap up before the input |
| AC7 | `crates/core/src/ui/help/tests.rs` :: `ac7_help_sections` (table), `ac7_help_custom_keymap`, `ac7_help_keys_and_texts`, `ac7_help_dim_rows` | titles, order, membership, keys, dim | stub returns one flat section |
| AC8 | `crates/core/src/ui/help/tests.rs` :: `ac8_help_keys` (table), `ac8_help_moves`, `ac8_help_filter`, `ac8_help_runs_row`, `ac8_help_runs_every_row`, `ac8_help_no_other_key_acts`, `ac8_help_text_inputs` | open, move, filter, run, close | stub `?` does nothing |
| AC9 | `crates/app/src/config.rs` :: `ac9_config_dir` (table) | resolved directory | stub always gives `~/.config/tidal-player` |
| AC10 | `crates/app/src/config.rs` :: `ac10_app_toml` (table), `ac10_load_app_toml` | values and messages | stub ignores unknown keys |
| AC11 | `crates/app/src/config.rs` :: `ac11_precedence` (table) + `crates/app/src/play.rs` :: `ac25_player_config` (unchanged) | winning value per source | stub lets the file beat the environment |
| AC12 | `crates/app/tests/config.rs` :: `ac12_broken_config_exits_2`, `ac12_missing_config_runs`, `crates/core/src/ui/keymap/tests.rs` :: `ac12_unsupported_notice`, `ac12_notice_after_welcome`, `crates/app/tests/config.rs` :: `ac12_unsupported_keymap_starts` | exit codes, stderr, notice | stub loader ignores the files |
| AC13 | `crates/app/src/input.rs` :: `ac18_key_events` (new rows) | decoded keys | stub drops Alt and F-keys (today's code) |
| AC14 | `crates/app/src/ui.rs` :: `ac14_help_80x24` (one snapshot per row), `ac14_help_40x12`, `ac14_library_percentages`, `ac17_no_panic_any_size` (new rows) | `contains` + snapshots | stub render draws no popup and keeps 40/40/20 |
| AC15 | `crates/app/src/main.rs` :: `ac15_tui_state_from_config` (table) + `crates/app/tests/daemon.rs` :: `ac15_daemon_reads_app_toml` | state and player settings from files | stub `tui_state` ignores the config |
| AC16 | `crates/app/tests/daemon.rs` :: `ac16_keymap_is_per_client` | each client's `n` | stub sends the default keymap's command |
| AC17 | `crates/app/tests/docs.rs` :: `ac17_every_command_documented` + reviewed at acceptance | names and default keys in `docs/config.md` | stub `docs/config.md` lacks the commands |
| AC18 | `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults`, `ac18_example_keymap_is_the_defaults` | every key present and equal to the default; one entry per default binding, builds to `Keymap::default()` | the example files are committed empty |

Checked by hand at acceptance on the user's machine (results in the PR description): copying a real spotify-player `keymap.toml` starts with the notice and its supported bindings working; the daemon restarted with `app.toml` setting `output_device` and `quality` plays to that device at that quality; `M-` and F-keys reach the TUI in the user's terminal.

## Crate placement

- `tidal-player-core::ui`: `keymap` (key syntax, `KeySequence`, `UiCommand` with parameters, `ActionBinding`, defaults, `build(entries) -> Result<Keymap, KeymapError>` with notices, lookup and prefix matching, labels; the entry types derive `Deserialize` so the binary deserialises TOML straight into them), `help` (sections, filter, the help popup's state and keys); `Key` gains `Alt`, `Left`, `Right`, `Home`, `End`, `Insert`, `Delete`, `F(u8)`; `State` holds the keymap and the pending sequence (replacing `pending_g`). No I/O, no new dependency
- `tidal-player`: `config` (directory resolution, reading both files, `app.toml` parsing and precedence), the `-c/--config-folder` flag, `input.rs` (new keys), rendering the help and the layout; `toml` added to `[workspace.dependencies]` (latest 1.x) and to the binary only
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-08, from code):

- spotify-player (`docs/config.md`, `spotify_player/src/key.rs`, `command.rs`, `config/keymap.rs`, `event/mod.rs`, master, read 2026-10-08): files in `$HOME/.config/spotify-player`, `-c/--config-folder`; `[[keymaps]]` `{command, key_sequence}`, `command = "None"` removes, parameterised commands as `{ VolumeChange = { offset = 1 } }`, `{ SeekForward = { duration = 10 } }`, `{ SeekBackward = { } }`; `[[actions]]` `{action, key_sequence, target}` with `PlayingTrack`/`SelectedItem`; key syntax `C-`/`M-` + `enter space tab backtab backspace esc left right up down insert delete home end page_up page_down f1…f12` or one non-space character; sequence matching: collected keys restart with the last key when they are no binding's prefix, and the first exact match runs (so a bound `g` hides `g …`, which it does not report). Its default keymap is the source of the names and keys in the table above; `layout.library = { playlist_percent, album_percent }`, default 40/40; `seek_duration_secs` is its seek setting
- tidalt (above): TOML files, unknown names as errors, broken files falling back to defaults with an in-UI error
- This repo: the eleven variables and their ranges (`crates/app/src/play.rs` `resolve_with`, 0003–0007); keys dispatched in `key_press` (`crates/core/src/ui/mod.rs`), `browse_key`, `popup_key`, `whole_list_key` (`browse.rs`), `search::input_key`/`window_key`; Alt combinations and F-keys dropped by `input::key_to_action`; the library's 40/40/20 split in `crates/app/src/ui/pages.rs`
- `toml` 1.1.7 is the latest release (`cargo info toml`, 2026-10-08)

Assumptions, checked at acceptance by hand: which `C-`/`M-` keys the user's terminal actually sends (crossterm's decoding is tested, the terminal is not).

## Decisions (answered by the user, 2026-10-08: 1–4 as proposed, then 5–8 "lgtm")

1. **Scope**: *answered: as proposed*: this spec is config files, keymap and keys help only; spotify-player's in-page filter (`/` popup on any page) and sorting (`s t`, `s a`, …), promised to "0008" by 0006 and 0007, move to their own spec (0012, "Filter and sort"), which binds them through this keymap. Alternative: all in 0008
2. **A broken file**: *answered: as proposed*: exit 2 before anything starts, in every mode (a daemon cannot show an in-UI error; the env variables already work this way). Alternative: tidalt's (run with defaults, show the error in the TUI, log it in the daemon)
3. **spotify-player names we do not have**: *answered: as proposed*: skip them with a one-line notice in the TUI, so a spotify-player `keymap.toml` can be copied as is; misspelt names stay errors. Alternative: every unknown name is an error
4. **A sequence that is a prefix of another**: *answered: as proposed*: an error naming both (spotify-player silently lets the shorter one win). Alternative: spotify-player's behaviour
5. **Precedence**: *answered: as proposed*: flag > environment > `app.toml` > default (0003–0007 promised the variables keep working). Alternative: the file beats the environment
6. **Text-input keys**: *answered: as proposed*: fixed in this spec (`Backspace`, `C-u`, `Enter`, `Esc`, …), not in the keymap. Alternative: spotify-player-style input commands in the keymap
7. **`app.toml` key names**: *answered: as proposed*: as in the table (spotify-player's `seek_duration_secs` and `[layout] library`, ours otherwise, `_secs` suffixes for durations)
8. **Client credentials** (0002 "may revisit in 0008"): *answered: as proposed*: stay constants, not in `app.toml`

## Out of scope

- The in-page filter (`/` on pages other than search) and sorting (decision 1)
- Live reload of either file; a command that prints the effective config
- Themes and colours (spotify-player's `theme.toml`), icons, border and progress-bar styles, `playback_format`
- The playback window's position and height (spotify-player's `[layout]` rest), the artist page's split
- spotify-player's count prefix (`5j`) and mouse support
- Configurable client credentials (decision 8)
- Remembering anything across runs (0009: the player's `remember_playback` key), MPRIS ([0010](0010-mpris.md), implemented: its `mpris` and `max_cover_arts` keys joined `app.toml`)

## Implementation notes (choices made where the spec was silent, 2026-10-08)

- **Config directory and flag**: `-c/--config-folder` is accepted after a subcommand (`tidal-player daemon -c DIR`, `tidal-player play -c DIR …`) and alone for the TUI (`tidal-player -c DIR`), not before a subcommand: the CLI's `args_conflicts_with_subcommands` reads `tidal-player -c DIR daemon` as items. `docs/config.md` says so. `login`, `logout`, `playback` and `daemon stop`/`unit` read no config; `devices` reads `app.toml` (its `*` marks `output_device`). The TUI, `daemon` and `play` validate both files; only TUI processes use the keymap
- **`app.toml`**: unknown nested keys are named by their dotted path (`unknown setting "layout.libary"`), checked in alphabetical order. A missing percentage in `[layout] library` falls back to 40 before the sum is checked. An empty `output_device` is refused. An empty `TIDAL_PLAYER_HIDE_VERSIONS` still means "hide nothing" (0006) and beats the file. A UTF-8 BOM is stripped before parsing so line and column stay right; only "not found" counts as a missing file
- **Keymap**: entry indices count from 0 (`keymaps[0]` is the first). `[[actions]]` are merged after `[[keymaps]]` (TOML loses the order between the two lists), so an action on a sequence replaces a command on it. The prefix-conflict message names every longer sequence with its command. The notice uses the singular for one name, deduplicated, in file order, actions included; it is kept in `State::notice` and shown after the first `Welcome` (which otherwise clears client messages). `GoToArtist` runs the first *Go to artist* entry. Popups now also take `G`/`end`, `g g`, `C-f`/`C-b`, `page_up`/`page_down` and `C-n`/`C-p`, per the table's "lists, popups". `State::pending_g` is kept as a mirror of the pending sequence (0004's `ac21_key_sequence` reads it)
- **Key decoding**: Control + Alt together and `f13` and above are dropped; Shift on named keys is ignored; Control + a named key (`C-enter`, `C-up`) now decodes as such instead of as the plain key, so 0004/0006's `ac18_key_events` rows for `F(1)`, `Home` and Alt-x were updated to this spec (they expected the keys dropped)
- **Keys help**: opened over a popup it lists only what acts there: the popup's section (`Popup · Actions`, … with its fixed keys such as `space`, `y`, `n` as rows without a command), `Lists` where the popup has a list, and `App` with `OpenCommandHelp`; the window, `Pages` and `Playback` sections are left out, as their keys do nothing under a popup (criterion "help that lies"). `FocusNextWindow`/`FocusPreviousWindow` are listed only on pages with several panes, `NextTab`/`PreviousTab` only where a pane has tabs. `?` does nothing during a whole-list load (only `Esc`/`q` act there). `Esc` while typing the filter clears it and ends typing; `Esc` with a set filter clears it, then closes. `Enter` on a row feeds its first key sequence through the normal key path with the original popup still open; a row without keys does nothing. At 80×24 the popup is 62×14 and scrolls with the highlight; the footer reads `/fav▏ · enter keep · esc clear` while typing and `/fav · enter run · esc clear` once set
- **Wiring**: `ui::LibraryLayout` lives in core (`config::LibraryLayout` re-exports it); `configured_tui_state(settings, keymap)` builds both the standalone and the attached TUI's state. `ac15_daemon_reads_app_toml` observes `page_size` and `quality` on the mock API; `release_paused_secs` is only shown to be accepted (observing a release needs an audio device). `ac16_keymap_is_per_client` runs two in-process client sessions against one real daemon (an attached TUI needs a terminal)
