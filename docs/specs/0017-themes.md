@docs/Eastereggs/67055c40c1e2.md

# 0017 — Themes and colours

- **Status**: approved (2026-10-24; amended 2026-10-24 by the user: AC16–AC19, decisions 8–11 and parts of AC14 are in the file named on the first line, kept out of the repository)
- **Owner**: tech-lead (primary session)
- **Issue**: [#20](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/20)
- **Depends on**: 0004 (implemented: the screen), 0006/0007/0011/0012/0013 (implemented: the pages, popups, filter and key hints this spec colours), 0008 (implemented: `app.toml`, its precedence and errors, the keymap and its notice, the keys help), 0014 (implemented: the Devices popup, the model for the Themes popup)
- **User docs**: [`docs/config.md`](../config.md) gains `theme.toml` and the `theme` key; [`docs/tui.md`](../tui.md) gains "Themes"; a new [`examples/theme.toml`](../../examples/theme.toml); their zh-TW copies (AC15)

## Context

The TUI draws in the terminal's own colours with three text attributes: bold for what has the focus or is playing, dim for what does not, reversed for the cursor, and red for errors (`crates/app/src/ui.rs`, `ui/pages.rs`). 0008 left themes out of scope, and `SwitchTheme` is one of the spotify-player commands its keymap skips.

spotify-player (`spotify_player/src/config/theme.rs`, `docs/config.md` "Themes", `examples/theme.toml`, master `7dc9d17`, read 2026-10-24) reads any number of `[[themes]]` from `theme.toml`, each a `name`, an optional `[themes.palette]` and an optional `[themes.component_style]`, and picks one with `theme = "<name>"` in `app.toml` or `-t/--theme`. `SwitchTheme` (`T`) opens a *Themes* popup in which moving the cursor applies the theme at once, `Enter` keeps it and `Esc` goes back to the one in use before. Its own default theme is colourful (magenta titles, cyan track, green progress bar). It ignores unknown fields silently.

tidalt had no themes.

### What could go wrong here, and the criterion that covers it

1. **The default look changes by accident**: moving every style into a theme must leave the screen as it is today → the built-in `default` theme is today's look, and every existing rendering and style test stays green unchanged (AC3)
2. **A copied spotify-player theme is refused, or its colours land somewhere else** → same file format, same colour names and parsing, its component names drawn where spotify-player draws them, and the ones we do not draw skipped with a notice (AC1, AC2, AC6). What a theme leaves out keeps today's look, not spotify-player's colourful defaults (decision 1); `examples/theme.toml` spells those out as a theme to copy from (AC14)
3. **A typo silently ignored** (`blok_title`, `Purpel`) → unknown fields, styles, colours and modifiers are errors naming the file, the theme and the field (AC2), as in 0008
4. **A theme hides the state**: a theme that sets the cursor row's colours must not lose the focus, playing and dim cues → those cues are added by the TUI on top of every theme (AC10)
5. **Rendering tests that cannot see colours**: today's snapshots are text only → this spec's snapshots record styles (AC9)
6. **A broken `theme.toml` stops the music**: the daemon draws nothing → only the TUI reads `theme.toml` (decision 5) (AC7)

## Behaviour

### `theme.toml`

In the config directory (0008 "Where the files are"), next to `app.toml` and `keymap.toml`; optional. spotify-player's format, so its `theme.toml` can be copied as is:

```toml
# ~/.config/tidal-player/theme.toml
[[themes]]
name = "dracula"
[themes.palette]
background = "#1e1f29"
foreground = "#f8f8f2"
red = "#ff5555"
cyan = "#8be9fd"
bright_black = "#555555"

[themes.component_style]
block_title = { fg = "Magenta", modifiers = ["Bold"] }
selection = { fg = "Black", bg = "Cyan" }
playback_progress_bar = { fg = "#bd93f9" }
```

A theme has:

- **`name`** (required, not empty): what `theme =` and the Themes popup call it
- **`palette`** (optional): `background`, `foreground`, and the sixteen terminal colours `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `bright_black`, `bright_red`, `bright_green`, `bright_yellow`, `bright_blue`, `bright_magenta`, `bright_cyan`, `bright_white`. A value is parsed as spotify-player parses it (ratatui's colour names): a name, ignoring case, spaces, `-` and `_`, with `bright` and `light`, `grey`, `gray` and `silver` as ratatui reads them (`red`, `Bright Black`, `light-red`, `dark_gray`, `reset`; note ratatui's `white` is the bright white and `gray` the plain one), a number `0`–`255` (the terminal's indexed colour), or `#rrggbb`. A colour left out is the terminal's own (the sixteen ANSI colours: `white` the plain one, ANSI 7). `background` and `foreground` left out leave the terminal's
- **`component_style`** (optional): one style per component (the table below), each an inline table of `fg`, `bg` (a palette name, `Black` … `White`, `BrightBlack` … `BrightWhite`, which takes that palette entry, or `#rrggbb`;; names ignoring case, `_`, `-` and spaces, so `bright_black` is `BrightBlack`) and `modifiers` (a list of `Bold`, `Dim`, `Italic`, `Underlined`, `RapidBlink`, `Reversed`, `Hidden`, `CrossedOut`, ignoring case). A style replaces that component's default whole: `selection = { fg = "Cyan" }` is cyan text without the default's reversed

The themes are the built-in **`default`** first, then the file's in file order. A theme named `default` in the file replaces the built-in one (in place, first); spotify-player instead ignores it (decision 6).

### Components

Where each component is drawn, and its default: the built-in `default` theme's style, which is today's look, and what any theme gets for a component it leaves out (decision 1). spotify-player's own default, for comparison (`examples/theme.toml` has it as the `spotify_player` theme, see AC14); `*` marks the two this player adds:

| Component | Drawn on | Default (today's look) | spotify-player's default |
|---|---|---|---|
| `block_title` | the title on every border: the frame's `tidal-player`, the windows and their tabs, the queue, the popups, the keys help, the key hints | — | `fg = "Magenta"` |
| `border` | every border | — | — |
| `playback_status` | the playback window's state symbol (`▶ ⏸ … ■`) and its indicators (`shuffle`, `repeat: …`, `autoplay`, the volume) | `Bold` | `Cyan`, `Bold` |
| `playback_track` | the track title (or `Nothing playing`) | `Bold` | `Cyan`, `Bold` |
| `playback_artists` | ` · ` and the artists after the title | `Bold` | `Cyan`, `Bold` |
| `playback_album` | the album row | — | `fg = "Yellow"` |
| `playback_metadata` | the details row (quality, format, device, bit-perfect) and the time `1:23 / 3:32` | — | `fg = "BrightBlack"` |
| `playback_progress_bar` | the bar's played part (`━`) | — | `fg = "Green"`, `bg = "BrightBlack"` |
| `playback_progress_bar_unfilled` | the bar's rest (`─`) | — | `bg = "BrightBlack"` |
| `current_playing` | the current entry's row in the queue | `Bold` | `Green`, `Bold` |
| `page_desc` | a page's title row (`Album · …`, `Search`, …) | `Bold` | `Cyan`, `Bold` |
| `playlist_desc` | a mix page's subtitle row | `Dim` | `BrightBlack`, `Dim` |
| `table_header` | the keys help's section headings (`Lists`, `Playback`, …) | `Bold` | `fg = "Blue"` |
| `selection` | the cursor row: lists, the queue, popups, the keys help, the search's top hit; and the active tab's name in a pane title | `Reversed` | `Reversed`, `Bold` |
| `secondary_row` | every other row of a list or the queue (the 2nd, 4th, …, counted in the list) | — | — |
| `like` | the `♥` on followed playlists | — | — |
| `error`* | the message row and failed loads (page, window, `Loading more…` that failed) | `fg = "Red"` | `fg = "Red"` |
| `prompt`* | the open prompt (`Add to queue: …`, `Play next: …`), the playlist-name prompt, the search input | `Bold` | `Bold` |

The palette's `background` and `foreground` are the style of the whole screen (spotify-player's `app` style): every cell starts in them, and the styles above are laid over.

spotify-player components this player does not draw (`playback_genres`, `lyrics_played`, `lyrics_playing`, `visualization`) are accepted and skipped: the TUI's message row says once at start `theme.toml: 2 spotify-player styles not used here: lyrics_played, lyrics_playing` (each name once, in file order), as 0008 does for the keymap. Any other name is an error.

### State cues on top of the theme

The TUI adds these to whatever the theme gives, in every theme (a style is *laid over* another as ratatui's `patch`: its colours replace the ones under it where set, its modifiers add):

- **Rows**: a row starts in the screen's style, or `secondary_row` on every other row; the queue's current entry has `current_playing` laid over; an unplayable track (0006), an autoplay suggestion and the `── Suggested` divider add `Dim`; the cursor row has `selection` laid over, and adds `Dim` when its window does not have the focus
- **Windows**: the focused window's title adds `Bold`; another window's title and border add `Dim`. Popup, keys-help and key-hint titles add `Bold`. The queue's title adds `Bold` while its filter is typed (0012). The frame's title adds nothing
- **The search input**: `prompt` while it has the focus; without it, `prompt` with `Bold` taken out, plus `Dim`
- **Status text** (`Loading…`, `Loading more…`, empty lists, `No rows match …`, popup statuses, the keys help's footer and dim rows, the key hints' dim rows): `Dim` added to the row's style

With the `default` theme this is exactly today's screen.

### Choosing the theme

`theme = "<name>"` in `app.toml` (default `"default"`), overridden by `TIDAL_PLAYER_THEME` and by `-t NAME`/`--theme NAME` on the TUI (`tidal-player -t dracula`), with 0008's precedence (flag > environment > file > default). An empty variable counts as unset. Each TUI uses its own: two TUIs attached to one daemon may use different themes; themes never travel through `Command`/`Event`.

Truecolor: `#rrggbb` colours are sent as they are (spotify-player does the same); a terminal without 24-bit colour shows what it makes of them (decision 3).

### The Themes popup (`T`)

`SwitchTheme` (spotify-player's name and default key `T`) opens the **Themes** popup from any page, as `D` opens Devices (0014). It is no longer one of the skipped spotify-player commands; its help text is `choose a theme`, in the keys help's `App` section.

```
┌Themes──────────────────────────────────────────┐
│● default                                       │
│  dracula                                       │
│  gruvbox_dark                                  │
└────────────────────────────────────────────────┘
```

- Every theme, in order (`default` first). `●` marks the theme in use when the popup opened; the cursor starts on it
- **Preview**: moving the cursor (the list commands, through the keymap) draws the whole TUI in the theme under the cursor at once
- `Enter` keeps the theme under the cursor and closes the popup; `Esc` (or `q`) goes back to the theme marked `●` and closes it
- The choice lasts for the run of that TUI: a restarted TUI uses `theme` again (decision 4). Only `theme.toml` and `app.toml` are read; nothing is written
- With no `theme.toml`, the popup lists `default` alone
- Popups keep 0006's rule: inside it only its own keys act; `T` in a text input types `T`; `T` with another popup open does nothing

### Errors

As 0008's (exit 2, nothing started, one line on stderr), from the TUI (standalone or attached) only (decision 5):

| Problem | Message |
|---|---|
| TOML syntax | `…/theme.toml: line 3, column 9: <parser message>` |
| Unknown field in a theme or its palette | `…/theme.toml: themes[1]: unknown field "colour"` / `…/theme.toml: themes[1].palette: unknown colour "orange"` (entries count from 0) |
| Unknown component | `…/theme.toml: themes[1].component_style: unknown style "blok_title"` |
| Unknown field in a style | `…/theme.toml: themes[1].component_style.border: unknown field "colour", expected fg, bg or modifiers` |
| Bad colour | `…/theme.toml: themes[1].palette.red: invalid colour "#ff55"` / `…/theme.toml: themes[1].component_style.border.fg: invalid colour "Purpel" (a palette name such as BrightBlack, or #rrggbb)` |
| Bad modifier | `…/theme.toml: themes[1].component_style.selection: unknown modifier "Blink"` |
| No name, an empty one, or one taken | `…/theme.toml: themes[3]: missing name` / `…: themes[3]: empty name` / `…: themes[3]: "dracula" is already themes[1]` |
| Unknown `theme` | `…/app.toml: unknown theme "draculla" (themes: default, dracula, gruvbox_dark)`; from the variable `TIDAL_PLAYER_THEME: unknown theme …`, from the flag `--theme: unknown theme …` |
| Wrong type of `theme` | `…/app.toml: invalid theme: expected a string, got 5` (every process, as any `app.toml` key) |
| A file that cannot be read | `…/theme.toml: <io error>` |

A missing `theme.toml` is no error. A UTF-8 byte-order mark and CRLF line ends read fine (0008).

## Acceptance criteria

Themes (`tidal_player_core::ui::theme`, pure; no TOML and no ratatui here: it takes the entries already deserialised and gives its own colour and style types):

- **AC1** — Colour syntax (table): palette values: every ratatui colour name, with case, spaces, `-`, `_`, `bright`/`light`, `grey`/`silver` variants, `reset`, `0` and `255`, `#rrggbb` upper and lower case; refused: `""`, `#fff`, `#ggg000`, `256`, `orange`. Style colours: the sixteen palette names in every accepted spelling and `#rrggbb`; refused: `Purpel`, `Reset`, `7`. Modifiers: the eight, ignoring case; refused: `Blink`. A test in `tidal-player` checks that the core's palette parsing gives what ratatui's `Color::from_str` gives on every row
- **AC2** — Building the theme list (table): no entries → `[default]`; entries follow `default` in file order; an entry named `default` replaces the built-in in first place; a partial palette keeps the ANSI colour of every entry left out; a component colour resolves through the theme's palette, hex directly; a component left out takes the "Default" column (today's look, decision 1), and `Red` in `error`'s default resolves through the theme's palette; a style set replaces the default whole; `background`/`foreground` give the screen style; every error under "Errors" for `theme.toml`, with its index and path; the four skipped spotify-player components → the notice list, each once, in order
- **AC3** — The `default` theme is today's look: it resolves to the "Default" column (table test), and every existing rendering and style test (`crates/app/src/ui.rs`, `ui/pages.rs` and their snapshots) runs **unchanged** and stays green with it
- **AC4** — Selecting the theme (table): default `default`; `app.toml`; the variable over the file; the flag over both; an empty variable falls through; an unknown name from each source, and an empty name, → the error naming the source and the known names
- **AC5** — The Themes popup (model, table): `T` opens it on every page with the cursor on the active theme and `●` on it; the list commands move and change `State`'s theme to the one under the cursor; `Enter` keeps it and closes; `Esc` and `q` restore the theme from before and close; no other key acts inside it; `T` types `T` in every text input and does nothing with another popup open; rebinding `SwitchTheme` in `keymap.toml` works; `SwitchTheme` is no longer in the unsupported list; the keys help lists `T  choose a theme` under `App` and a `Popup · Themes` section when opened from the popup
- **AC6** — Notices: a `theme.toml` with skipped components gives the TUI's start notice; with a keymap notice too, both are shown, the keymap's first, joined by ` · `

Loading (`tidal-player`):

- **AC7** — Files (`assert_cmd`, a temp config dir): no `theme.toml` runs with `default`; a broken `theme.toml`, or an unknown `theme` in `app.toml`, makes `tidal-player` exit 2 with the message on stderr before raw mode (clean stderr, no escape sequences); `tidal-player daemon` and `tidal-player play …` start with the same broken `theme.toml` and unknown `theme` (they do not read it); `theme = 5` makes all three exit 2
- **AC8** — Style mapping (table): the core's colours (each ANSI colour, indexed, RGB, reset) and modifiers map to ratatui's

Rendering (`tidal-player`, `insta` snapshots of the buffer **with its styles** — the `TestBackend` buffer's `Debug`, reviewed by eye — plus cell assertions):

- **AC9** — Components: with a test theme giving every component and `background`/`foreground` a distinct colour, at 80×24, a styled snapshot each of: the queue page playing (playback window, current entry, a suggestion, the cursor, every other row), the library with *Albums* focused (focused and unfocused titles and borders, a tab name, `♥`, an unfocused cursor), a mix page (subtitle), the search page with the input focused and with the top hit selected, a failed page, the actions popup, the keys help, the Themes popup, the key hints. For each component a cell assertion finds its colour where the table says it is drawn; cells drawn by nothing have `background`/`foreground`
- **AC10** — Cues on top (table, cell assertions with a theme whose `selection`, `block_title`, `border`, `current_playing` and `prompt` set colours only): the unfocused window's title and border dim; an unplayable row dim; the cursor row in an unfocused window dim; the focused window's and popups' titles bold; the queue's title bold while its filter is typed; the search input bold when focused and dim, not bold, when not
- **AC11** — Preview: moving the cursor in the Themes popup and drawing gives the frame in the theme under the cursor; `Esc` and drawing gives the first theme's frame again (cell assertions)
- **AC12** — No panic from 0×0 to 120×40 with the AC9 test theme (0004 AC17's test, new rows)

Wiring:

- **AC13** — The TUI's `State` gets its theme list and active theme from the files (`configured_tui_state`, table: defaults; a `theme.toml` and `theme`; the variable over the file); the attached and standalone TUIs build it the same way, so two attached TUIs with different config dirs use different themes. `Command` and `Event` (`tidal_player_core::protocol`) are unchanged (checked at acceptance: `git diff` on `protocol.rs` is empty)

Docs and examples:

- **AC14** — `examples/theme.toml` holds a `spotify_player` theme setting every component to the "spotify-player's default" column (the block to copy for spotify-player's look) as far as [`docs/Eastereggs/67055c40c1e2.md`](../Eastereggs/67055c40c1e2.md) allows, then spotify-player's sample themes it allows (its `examples/theme.toml`, MIT, with a line crediting it), and loads with no error and no notice (test, which also checks the `spotify_player` theme against the column); `examples/app.toml` gains `theme = "default"` (0008 AC18's test keeps passing)
- **AC16**–**AC19** — in [`docs/Eastereggs/67055c40c1e2.md`](../Eastereggs/67055c40c1e2.md)
- **AC15** — `docs/config.md` documents `theme.toml` (format, palette, every component with where it is drawn and both defaults, colours, modifiers, the cues, the errors, the skipped spotify-player names), the `theme` key, `TIDAL_PLAYER_THEME` and `--theme`, and drops `SwitchTheme` from the unsupported commands; `docs/tui.md` gains "Themes" and the `T` key; `README.md` mentions themes; zh-TW copies of each. A test checks that every component appears in `docs/config.md` (and 0008 AC17's checks `SwitchTheme` and `T`). `CLAUDE.md` "Features" gains the entry

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| No `theme.toml`, no `theme` | `default`: today's screen |
| `theme.toml` with no `[[themes]]` (empty, comments only) | `default` alone; no error |
| A theme with neither palette nor styles | Today's look, as `default` |
| A palette-only theme (spotify-player's `dracula`) | Today's look in its `background`, `foreground` and `red` (errors): the other palette colours are used only by styles that name them (decision 1) |
| A theme whose `selection` matches the rows' colours | The cursor is invisible; that theme's problem, as in spotify-player (no correction) |
| `theme.toml` broken, daemon running | The daemon plays on; the TUI refuses to start with the message (decision 5) |
| `theme` names a theme the attached TUI's `theme.toml` lacks | That TUI exits 2; the daemon and other TUIs are unaffected |
| A terminal with 16 or 256 colours | Hex colours sent as they are (decision 3) |
| `NO_COLOR` set | Not read (out of scope) |
| The album cover (0016) | Drawn as before; the theme does not touch it |
| The Themes popup in a tiny terminal | Clipped like every popup; no panic (AC12) |
| Themes popup open when the player disconnects | The popup stays; themes are the TUI's own |

## Test plan

Each test is named after its criterion (`ac5_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0008.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/theme/tests.rs` :: `ac1_palette_colours`, `ac1_style_colours`, `ac1_modifiers` (tables) + `crates/app/src/ui/theme.rs` :: `ac1_palette_matches_ratatui` | parsed values and refusals; agreement with ratatui | stub parsers accept `#rrggbb` only |
| AC2 | `crates/core/src/ui/theme/tests.rs` :: `ac2_build_themes` (table) | order, replacement, fallbacks, errors, notices | stub `build` returns `[default]` and ignores entries |
| AC3 | `crates/core/src/ui/theme/tests.rs` :: `ac3_default_is_todays_look` + the existing tests in `crates/app/src/ui.rs` (unchanged) | the default column; old screens | stub `default` theme is empty, so `ac3_default_is_todays_look` fails (the old tests are the refactor's safety net, not its red) |
| AC4 | `crates/app/src/config.rs` :: `ac4_theme_precedence` (table) | the chosen name or the error | stub always picks `default` |
| AC5 | `crates/core/src/ui/theme/tests.rs` :: `ac5_themes_popup` (table), `ac5_switch_theme_key`, `ac5_help_lists_switch_theme` | popup state, active theme, keys | stub `SwitchTheme` does nothing |
| AC6 | `crates/core/src/ui/theme/tests.rs` :: `ac6_theme_notice`, `ac6_both_notices` | notice text | stub drops the theme notice |
| AC7 | `crates/app/tests/config.rs` :: `ac7_broken_theme_exits_2`, `ac7_unknown_theme_exits_2`, `ac7_daemon_ignores_theme_toml`, `ac7_theme_wrong_type_exits_2`, `ac7_missing_theme_toml_runs` | exit codes, stderr | stub loader ignores `theme.toml` |
| AC8 | `crates/app/src/ui/theme.rs` :: `ac8_style_mapping` (table) | ratatui colour and modifiers | stub maps every colour to `Reset` |
| AC9 | `crates/app/src/ui.rs` :: `ac9_theme_queue_80x24`, `ac9_theme_library_80x24`, `ac9_theme_mix_80x24`, `ac9_theme_search_80x24`, `ac9_theme_failed_80x24`, `ac9_theme_popups_80x24`, `ac9_theme_components` | styled snapshots; each component's colour where drawn | stub render ignores the theme |
| AC10 | `crates/app/src/ui.rs` :: `ac10_cues_on_top` (table) | dim/bold per cue | stub render applies the theme style alone |
| AC11 | `crates/app/src/ui.rs` :: `ac11_preview_draws_theme` | cells in the previewed theme | stub render reads the configured theme, not the previewed one |
| AC12 | `crates/app/src/ui.rs` :: `ac17_no_panic_any_size` (new rows) | no panic | none: new rows of an existing safety-net test, which pass on the stubs too (a stub cannot panic where the real code might); they guard the green code |
| AC13 | `crates/app/src/main.rs` :: `ac13_tui_state_theme` (table) | `State`'s themes and active theme | stub `configured_tui_state` ignores the themes |
| AC14 | `crates/app/tests/examples.rs` :: `ac14_example_theme_toml_loads`, `ac18_example_app_toml_is_the_defaults` (existing) | loads, no notice; `theme` present | the example file is committed empty |
| AC16–AC19 | in [`docs/Eastereggs/67055c40c1e2.md`](../Eastereggs/67055c40c1e2.md) | | |
| AC15 | `crates/app/tests/docs.rs` :: `ac15_every_component_documented` + `ac17_every_command_documented` (existing) + reviewed at acceptance | names in `docs/config.md` | the docs lack the components |

Checked by hand at acceptance on the user's machine (results in the PR description): copying spotify-player's `examples/theme.toml` and switching through its themes with `T` in the user's terminal; a theme set with `theme =` in `app.toml` in a TUI attached to the daemon while another attached TUI keeps `default`.

## Crate placement

- `tidal-player-core::ui::theme`: `Color` (reset, the sixteen ANSI colours, indexed, RGB), `Modifiers`, `Style` (`fg`, `bg`, `modifiers`, `patch`), `Component`, `Theme` (name, screen style, one resolved `Style` per component), the entry types (`ThemeEntry`, palette and style entries, deriving `Deserialize` so the binary deserialises TOML straight into them, unknown names kept for the checks above), `build(entries) -> Result<Themes, ThemeError>` with notices, the colour and modifier parsers; `State` holds the themes, the active one and the popup's (`Popup::Themes`); `UiCommand::SwitchTheme` with default key `T`. No I/O, no new dependency
- `tidal-player`: `config` (reading `theme.toml`, the `theme` key, `TIDAL_PLAYER_THEME`, `-t/--theme`), `ui::theme` (core styles → ratatui styles), rendering every component through the active theme
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-24):

- spotify-player (above, master `7dc9d17`): the `theme.toml` fields, the palette defaults (the sixteen ANSI colours; `white` is ratatui's `Gray`, `bright_black` `DarkGray`, `bright_white` `White`), palette values through ratatui's `Color::from_str`, component colours as `Black` … `BrightYellow` (exact case) or `#rrggbb`, the eight modifiers, every component and its default as in the "spotify-player's default" column, unknown fields ignored, a file theme named like an existing one skipped, `SwitchTheme`'s popup (current theme first, the cursor applies, `Enter` keeps, `Esc` returns to the first), `secondary_row` used for the help table's alternate rows only, `theme`/`-t` to choose
- ratatui-core 0.1.2 `Color::from_str` (the version this workspace builds): lower-cases, drops spaces, `-` and `_`, maps `bright`→`light`, `grey`/`silver`→`gray`, `lightblack`→`darkgray`, `lightwhite`/`lightgray`→`white`; then a name, a `u8` index, or `#rrggbb`
- This repo: every style in the TUI is in `crates/app/src/ui.rs` and `ui/pages.rs` (bold, dim, reversed, red), the snapshots record text only, `T` is unbound, `SwitchTheme` is in `UNSUPPORTED_COMMANDS`, `tidal-player-core` may not depend on ratatui (`cargo xtask layering`)

Assumptions: none the tests rely on. Whether the user's terminal shows hex colours and `RapidBlink` is checked by hand at acceptance.

## Decisions (answered by the user, 2026-10-24: 1 as the alternative, 2–7 as proposed)

1. **What a `theme.toml` theme leaves out**: *answered: the alternative*: today's look for every theme, so a palette-only theme such as spotify-player's `dracula` changes only the background, foreground and the red of errors; spotify-player's default component styles are in `examples/theme.toml` to copy. Proposed was spotify-player's defaults for the components a file theme leaves out
2. **Icons, border and progress-bar styles** (`play_icon`, `pause_icon`, `liked_icon`, `border_type`, `progress_bar_type`): *answered: as proposed*: their own spec. Alternative: in this one
3. **Truecolor**: *answered: as proposed*: hex colours sent as they are (spotify-player's behaviour). Alternative: approximate them to 256 colours when `COLORTERM` does not say `truecolor`/`24bit`
4. **Remembering a theme picked with `T`**: *answered: as proposed*: no, it lasts for the run, as a device picked with `D` (0014); `theme` in `app.toml` makes it permanent. Alternative: remember it per TUI in the state directory (0009)
5. **Who reads `theme.toml`**: *answered: as proposed*: the TUI only (a daemon draws nothing, and a typo in a colour should not stop the music); `theme` in `app.toml` is still type-checked by every process. Alternative: every process validates `theme.toml`, as 0008 does for `keymap.toml`
6. **Names**: *answered: as proposed*: a file theme named `default` replaces the built-in; two file themes with one name are an error; unknown names are errors except spotify-player's four components we do not draw. Alternative: spotify-player's (the first of a name wins, unknown fields ignored)
7. **Two components of our own**: *answered: as proposed*: `error` (today red) and `prompt` (today bold), as spotify-player has neither; `table_header` colours the keys help's headings (we have no table headers). Alternative: keep errors and prompts out of themes

Decisions 8–11 are in [`docs/Eastereggs/67055c40c1e2.md`](../Eastereggs/67055c40c1e2.md)

## Out of scope

- Icons, border type, progress-bar type (decision 2)
- Approximating hex colours (decision 3); reading `NO_COLOR`
- Remembering the theme picked with `T` (decision 4)
- Live reload of `theme.toml`; a command that lists the themes
- Marking the playing track in lists other than the queue (spotify-player's `current_playing` everywhere)
- spotify-player's default component styles as the fallback (decision 1)
- spotify-player's `theme_parse` script and themes built from terminal colour schemes
