English | [正體中文](zh-TW/config.md)

# Configuration

Three optional files configure tidal-player: **`app.toml`** for the settings, **`keymap.toml`** for the keys and **`theme.toml`** for the colours, in [spotify-player](https://github.com/aome510/spotify-player)'s format so its `[[keymaps]]` and `[[themes]]` blocks can be copied across. Design: [spec 0008](specs/0008-keymap-and-config.md) and [spec 0017](specs/0017-themes.md) (themes). Only the TUI reads `theme.toml`; the player and the daemon never do.

## Where the files are

The **config directory** is, first found:

1. `-c DIR` / `--config-folder DIR`
2. `$TIDAL_PLAYER_CONFIG_DIR`
3. `$XDG_CONFIG_HOME/tidal-player`
4. `~/.config/tidal-player`

An empty variable counts as unset. The flag goes **after** the subcommand: `tidal-player daemon -c DIR`, `tidal-player play -c DIR 123`, and plain `tidal-player -c DIR` for the TUI.

The directory and all three files may be missing: the defaults are used, and nothing is ever created or written there. The files are read **once, at start**; a daemon picks up a changed file when restarted (`systemctl --user restart tidal-player`).

**Starting points**: [`examples/app.toml`](../examples/app.toml) and [`examples/keymap.toml`](../examples/keymap.toml) spell out every default (each setting with its range, each key binding with its command), and [`examples/theme.toml`](../examples/theme.toml) holds spotify-player's look and some of its sample themes to copy from. Copied as they are they change nothing; edit what you want and delete the rest:

```sh
mkdir -p ~/.config/tidal-player
cp examples/app.toml examples/keymap.toml examples/theme.toml ~/.config/tidal-player/
```

Each process uses what concerns it: the player (the standalone TUI, the daemon, `play`) the player settings, the TUI (standalone or attached to a daemon) the TUI settings, `keymap.toml` and `theme.toml`. An attached TUI uses its own `keymap.toml`, `theme.toml` and TUI settings, not the daemon's. Still, every process that starts a player or a TUI checks `app.toml` and `keymap.toml` whole: a typo in a key stops the daemon too, so one restart shows it. `theme.toml` is checked by the TUI only, so a typo in a colour never stops the music.

## `app.toml`

Flat keys, every one optional.

| Key | Accepted | Default | Overridden by | Read by |
|---|---|---|---|---|
| `quality` | `"hi-res"`, `"lossless"`, `"high"` | `"hi-res"` | `--quality`, `TIDAL_PLAYER_QUALITY` | player |
| `output_device` | any ALSA PCM name (see `tidal-player devices`) | `"default"` | `--device`, `TIDAL_PLAYER_DEVICE` | player |
| `volume_step` | integer 1–25 (%) | `5` | `TIDAL_PLAYER_VOLUME_STEP` | TUI |
| `seek_duration_secs` | integer 1–600 | `5` | `TIDAL_PLAYER_SEEK_STEP` | TUI |
| `previous_restart_secs` | integer 0–60 | `3` | `TIDAL_PLAYER_PREVIOUS_RESTART` | player |
| `autoplay` | `true`, `false` | `false` | `--autoplay`, `TIDAL_PLAYER_AUTOPLAY` | player |
| `release_paused_secs` | integer 0–3600, or `"never"` | `10` | `TIDAL_PLAYER_RELEASE_PAUSED` | player |
| `remember_playback` | `true`, `false` | `true` | `TIDAL_PLAYER_REMEMBER_PLAYBACK` (`on`, `off`) | player |
| `mpris` | `true`, `false` | `true` | `TIDAL_PLAYER_MPRIS` (`on`, `off`) | player |
| `max_cover_arts` | integer 0–1000 (`0`: no cover cache) | `20` | `TIDAL_PLAYER_MAX_COVER_ARTS` | player |
| `theme` | a theme's name | `"default"` | `-t`/`--theme` (TUI), `TIDAL_PLAYER_THEME` | TUI |
| `key_hints` | `true`, `false` | `true` | `TIDAL_PLAYER_KEY_HINTS` (`on`, `off`) | TUI |
| `key_hints_delay_ms` | integer 0–10 000 (ms; `0`: at once) | `1000` | `TIDAL_PLAYER_KEY_HINTS_DELAY_MS` | TUI |
| `confirm_removals` | `true`, `false` | `true` | `TIDAL_PLAYER_CONFIRM_REMOVALS` (`on`, `off`) | TUI |
| `cover_img` | `true`, `false` | `true` | `TIDAL_PLAYER_COVER_IMG` (`on`, `off`) | TUI |
| `cover_img_rows` | integer 4–20 (the playback window's rows with the cover) | `6` | `TIDAL_PLAYER_COVER_IMG_ROWS` | TUI |
| `cover_img_protocol` | `"auto"`, `"sixel"`, `"kitty"`, `"iterm2"`, `"halfblocks"` | `"auto"` | `TIDAL_PLAYER_COVER_IMG_PROTOCOL` | TUI |
| `page_size` | integer 1–10 000 | `100` | `TIDAL_PLAYER_PAGE_SIZE` | player, TUI |
| `search_page_size` | integer 1–1000 | `20` | `TIDAL_PLAYER_SEARCH_PAGE_SIZE` | player, TUI |
| `hide_versions` | array of strings (`[]` hides nothing) | the sixteen words of [Settings](playback.md#settings) | `TIDAL_PLAYER_HIDE_VERSIONS` | player |
| `[layout] library = { playlist_percent, album_percent }` | integers 1–98 each, sum at most 99; *Artists* takes the rest | `40`, `40` | | TUI |

What each setting does is described in [Settings](playback.md#settings); `output_device` is the device a player starts on: one chosen while it runs (`D`, `tidal-player playback device`) lasts for that run only, so set `output_device` to keep it (see [Choosing the output device](playback.md#choosing-the-output-device)); the library layout in [Windows](tui.md#windows); `key_hints` and `key_hints_delay_ms` in [Key hints](tui.md#key-hints); `cover_img`, `cover_img_rows` and `cover_img_protocol` in [Album cover](tui.md#album-cover); `mpris` and `max_cover_arts` in [Desktop controls and media keys](mpris.md); `theme` names one of the themes in [`theme.toml`](#themetoml) (`default` when there is none), and `T` switches it while the TUI runs (see [Themes](tui.md#themes)). The album covers are kept in the **cache directory**: `$TIDAL_PLAYER_CACHE_DIR`, else `$XDG_CACHE_HOME/tidal-player`, else `~/.cache/tidal-player` (see [Album covers](mpris.md#album-covers)).

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

**Precedence**, per setting: flag > environment variable > `app.toml` > default. When the environment and the file disagree, the environment wins silently. `hide_versions` matches as the variable does (lower-cased, ignoring spaces, hyphens, underscores, dots and `+`).

## `theme.toml`

The colours of the TUI, in spotify-player's format: any number of `[[themes]]`, each with a `name`, an optional `[themes.palette]` and an optional `[themes.component_style]`. A spotify-player `theme.toml` can be copied as is. Only the TUI reads it, so a typo in a colour stops `tidal-player` but never the daemon or `play`. Design: [spec 0017](specs/0017-themes.md).

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

Pick the theme with `theme = "dracula"` in `app.toml`, `TIDAL_PLAYER_THEME=dracula` or `tidal-player -t dracula` (flag > environment > file > default; an empty variable counts as unset), or switch while the TUI runs with `T` (see [Themes](tui.md#themes)). Each TUI uses its own: two TUIs attached to one daemon may use different themes.

- **`name`** (required, not empty): what `theme =` and the Themes popup call the theme
- **`palette`** (optional): `background`, `foreground`, and the sixteen terminal colours `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `bright_black`, `bright_red`, `bright_green`, `bright_yellow`, `bright_blue`, `bright_magenta`, `bright_cyan`, `bright_white`. A value is a colour name, ignoring case, spaces, `-` and `_` (`red`, `Bright Black`, `light-red`, `dark_gray`, `reset`; `bright` and `light` mean the same, and so do `grey` and `gray`), a number `0`–`255` (the terminal's indexed colour), or `#rrggbb`. Mind ratatui's names: `white` is the **bright** white and `gray` the plain one (ANSI 7). A colour left out is the terminal's own, and so are `background` and `foreground` left out
- **`component_style`** (optional): one style per component (the table below), each an inline table of `fg`, `bg` and `modifiers`. `fg` and `bg` are a palette name (`Black` … `White`, `BrightBlack` … `BrightWhite`, ignoring case, `_`, `-` and spaces, which takes that colour from the theme's palette) or `#rrggbb`. `modifiers` is a list of `Bold`, `Dim`, `Italic`, `Underlined`, `RapidBlink`, `Reversed`, `Hidden`, `CrossedOut`, ignoring case. A style **replaces** the component's default whole: `selection = { fg = "Cyan" }` is cyan text without the default's reversed

`#rrggbb` colours are sent to the terminal as they are; a terminal without 24-bit colour shows what it makes of them.

### Themes order

The themes are the built-in **`default`** first, then the file's in file order; the Themes popup lists them so. `default` is today's look. A theme named `default` in the file replaces the built-in one (in first place). Two file themes with one name are an error. With no `theme.toml`, or none with a `[[themes]]`, `default` is the only theme.

### Components

Where each component is drawn, its default (the `default` theme's look, and what any theme gets for a component it leaves out) and, for comparison, spotify-player's own default. `*` marks the two this player adds.

| Component | Drawn on | Default | spotify-player's default |
|---|---|---|---|
| `block_title` | the title on every border: the frame's `tidal-player`, the windows and their tabs, the queue, the popups, the keys help, the key hints | none | `fg = "Magenta"` |
| `border` | every border | none | none |
| `playback_status` | the playback window's state symbol (`▶ ⏸ … ■`) and its indicators (`shuffle`, `repeat: …`, `autoplay`, the volume) | `Bold` | `Cyan`, `Bold` |
| `playback_track` | the track title (or `Nothing playing`) | `Bold` | `Cyan`, `Bold` |
| `playback_artists` | ` · ` and the artists after the title | `Bold` | `Cyan`, `Bold` |
| `playback_album` | the album row | none | `fg = "Yellow"` |
| `playback_metadata` | the details row (quality, format, device, bit-perfect) and the time `1:23 / 3:32` | none | `fg = "BrightBlack"` |
| `playback_progress_bar` | the bar's played part (`━`) | none | `fg = "Green"`, `bg = "BrightBlack"` |
| `playback_progress_bar_unfilled` | the bar's rest (`─`) | none | `bg = "BrightBlack"` |
| `current_playing` | the current entry's row in the queue | `Bold` | `Green`, `Bold` |
| `page_desc` | a page's title row (`Album · …`, `Search`, …) | `Bold` | `Cyan`, `Bold` |
| `playlist_desc` | a mix page's subtitle row | `Dim` | `BrightBlack`, `Dim` |
| `table_header` | the keys help's section headings (`Lists`, `Playback`, …) | `Bold` | `fg = "Blue"` |
| `selection` | the cursor row: lists, the queue, popups, the keys help, the search's top hit; and the active tab's name in a pane title | `Reversed` | `Reversed`, `Bold` |
| `secondary_row` | every other row of a list or the queue (the 2nd, 4th, …, counted in the list) | none | none |
| `like` | the `♥` on followed playlists | none | none |
| `error`* | the message row and failed loads (a page, a window, a `Loading more…` that failed) | `fg = "Red"` | `fg = "Red"` |
| `prompt`* | the open prompt (`Add to queue: …`, `Play next: …`), the playlist-name prompt, the search input | `Bold` | `Bold` |

A component a theme leaves out keeps today's look, **not** spotify-player's colourful defaults: a palette-only theme such as spotify-player's `dracula` changes only the background, the foreground and the red of errors. To get spotify-player's look, copy the `spotify_player` theme in [`examples/theme.toml`](../examples/theme.toml), which spells its defaults out, and edit it. The palette's `background` and `foreground` are the style of the whole screen: every cell starts in them and the styles above are laid over.

spotify-player components this player does not draw (`playback_genres`, `lyrics_played`, `lyrics_playing`, `visualization`) are accepted and skipped, so its themes can be copied as is. The TUI's message row says once at start which ones: `theme.toml: 2 spotify-player styles not used here: lyrics_played, lyrics_playing` (each name once, in file order; after the keymap's notice, if any, joined by ` · `). Any other name is an error.

### What the TUI adds on top

Whatever the theme, the TUI adds these state cues over the component styles, so a theme cannot hide them (a style laid over another replaces its colours where set and adds its modifiers):

- **Rows**: a row starts in the screen's style, or `secondary_row` on every other row; the queue's current entry has `current_playing` laid over; an unplayable track, an autoplay suggestion and the `── Suggested` divider add `Dim`; the cursor row has `selection` laid over and adds `Dim` when its window does not have the focus
- **Windows**: the focused window's title adds `Bold`; another window's title and border add `Dim`. Popup, keys-help and key-hint titles add `Bold`, and so does the queue's title while its filter is typed. The frame's title adds nothing
- **The search input**: `prompt` with `Bold` added while it has the focus; without it, `prompt` without `Bold`, plus `Dim`
- **Status text** (`Loading…`, `Loading more…`, empty lists, `No rows match …`, popup statuses, the keys help's footer and dim rows, the key hints' dim rows): `Dim` added to the row's style

With the `default` theme this is exactly the TUI's look without themes.

## `keymap.toml`

Entries that **add to or replace** the default keys below.

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

- **`[[keymaps]]`**: `key_sequence` and `command`. `command` is a command name, `"None"` (removes whatever that sequence is bound to), or a one-entry inline table for a command with parameters. An entry replaces the binding of the **same** sequence only: a command keeps its other keys (above, `n` still skips). A later entry beats an earlier one
- **`[[actions]]`**: `key_sequence`, `action` and an optional `target`: `"SelectedItem"` (the default: the selected row) or `"PlayingTrack"`. The key runs that entry of the [actions popup](tui.md#actions) directly, without opening it. Where the popup would not list it (*Go to album* on an artist, *Play next* on the playing track, anything with `PlayingTrack` while nothing plays) it does nothing
- A command left with no key is unreachable and not in the keys help. `OpenCommandHelp` may be unbound; `Quit` must keep at least one key

### Key syntax

A **key sequence** is keys separated by single spaces (`g g`, `s l a`). A key is:

- a single character: `a`, `A` (Shift-a), `>`, `?`, `-`
- a name: `enter`, `space`, `tab`, `backtab` (Shift-Tab), `backspace`, `esc`, `left`, `right`, `up`, `down`, `insert`, `delete`, `home`, `end`, `page_up`, `page_down`, `f1` … `f12`
- either of those after `C-` (Control) or `M-` (Alt): `C-s`, `M-enter`, `C-space`, `M-p`. `C-S` is `C-s`: terminals report Control-letters without case

Pressed keys collect: when what you pressed is no binding's start, collecting restarts with the last key alone (`g x` does what `x` does); when it is a binding, that binding runs. There is no timeout. A sequence that is the start of another cannot be finished, so two sequences of your file in that relation (`s` and `s l a`) are refused, see [Errors](#errors). A sequence of your file that conflicts with a **default** wins: the default is dropped, and the message row says so once at start, with the notice of skipped names below (joined by ` · `): `keymap.toml: "d" replaces the default "d d" (RemoveSelectedItem)`, or `keymap.toml: "g" replaces the defaults "g g" (SelectFirstOrScrollToTop), "g l" (LibraryPage), …` (all of them, in the defaults' order; one clause per sequence of yours, joined by `;`). So a `keymap.toml` copied from an earlier example, with `RemoveFromQueue` on `d`, still works next to the default `d d`.

**Unreliable keys**: many terminals cannot send `C-enter`, `C-tab`, `C-backspace` or Control with the arrows as distinct keys; such a binding is accepted but may never fire. A terminal sends `C-i` as `tab`, `C-m` as `enter` and `C-[` as `esc`, so bind those names instead. `M-` keys need a terminal that sends Alt as Escape + key (most do; on macOS, "Use Option as Meta"). The keys help (`?`) shows what is bound; pressing the key shows whether it arrives.

### Where keys act

A binding acts only where its command does (the "Acts in" column below); elsewhere it does nothing. In a popup only its own keys act: the list commands, `ChooseSelected`, `ClosePopup` and `OpenCommandHelp`.

Some keys are **fixed**, not in the keymap: text inputs (the `o`/`O` prompt, the search input, the playlist-name prompt, a window's filter while you type it) take every printable character and paste before the keymap, with their editing keys `backspace`, `C-u`, `enter`, `esc` (in the search input also `tab`/`backtab`, `C-c` quits and `C-q` goes back; in a filter `up`, `down`, `page_up`, `page_down` move over the matching rows and `C-c` quits); the role filter's `space`; the questions' `y` and `n`; the keys help's `/` filter.

### Commands

| Command | Default keys | Acts in | Help text |
|---|---|---|---|
| `ResumePause` | `space` | everywhere | play / pause |
| `NextTrack` | `n` | everywhere | next track |
| `PreviousTrack` | `p` | everywhere | previous track |
| `SeekForward` (`duration`: 1–600 s, optional) | `>` | everywhere | seek forward (by `duration`, default `seek_duration_secs`) |
| `SeekBackward` (`duration`: 1–600 s, optional) | `<` | everywhere | seek backward |
| `SeekStart` | `^` | everywhere | back to the start of the track |
| `Shuffle` | `C-s` | everywhere | shuffle on / off |
| `Repeat` | `C-r` | everywhere | repeat: off → queue → track |
| `ToggleAutoplay` | `A` | everywhere | autoplay on / off |
| `SwitchDevice` | `D` | everywhere | choose the output device (the devices popup) |
| `SwitchTheme` | `T` | everywhere | choose a theme |
| `VolumeUp` | `+` | everywhere | volume up by `volume_step` |
| `VolumeDown` | `-` | everywhere | volume down by `volume_step` |
| `VolumeChange` (`offset`: −25…25, not 0) | none | everywhere | volume by `offset` % |
| `Mute` | `_` | everywhere | mute / unmute |
| `AddToQueuePrompt` | `o` | everywhere | add a link or ID to the end of the queue |
| `PlayNextPrompt` | `O` | everywhere | add a link or ID to play next |
| `SelectNextOrScrollDown` | `j`, `down`, `C-n` | lists, popups | move down |
| `SelectPreviousOrScrollUp` | `k`, `up`, `C-p` | lists, popups | move up |
| `PageSelectNextOrScrollDown` | `C-f`, `page_down` | lists, popups | move down a window |
| `PageSelectPreviousOrScrollUp` | `C-b`, `page_up` | lists, popups | move up a window |
| `SelectFirstOrScrollToTop` | `g g` | lists, popups | move to the top |
| `SelectLastOrScrollToBottom` | `G`, `end` | lists, popups | move to the last loaded row |
| `ChooseSelected` | `enter` | lists, popups | play the track with its list / open / run |
| `AddSelectedItemToQueue` | `Z`, `C-z` | lists | add to the end of the queue |
| `RemoveSelectedItem`* | `d d` | the queue, your playlists' tracks, favorite tracks, the library's lists | remove from the list (asks `y/n` first; [what it removes](tui.md#confirming-removals)) |
| `RemoveFromQueue` | | the queue | remove from the queue (asks `y/n` first; no default key; `RemoveSelectedItem` does it) |
| `ShowActionsOnSelectedItem` | `g a`, `C-space` | lists | actions on the selected row |
| `ShowActionsOnCurrentTrack` | `a` | everywhere | actions on the playing track |
| `FocusNextWindow` | `tab` | pages | next pane |
| `FocusPreviousWindow` | `backtab` | pages | previous pane |
| `NextTab` | `]` | panes with tabs | next tab |
| `PreviousTab` | `[` | panes with tabs | previous tab |
| `RoleFilter` | `f` | the artist's *All tracks* | the role filter |
| `Queue` | `z` | everywhere | the queue page |
| `LibraryPage` | `g l` | everywhere | the library |
| `LikedTrackPage` | `g y` | everywhere | favorite tracks |
| `SearchPage` | `g s` | everywhere | the search page (on one: its input) |
| `MixesPage` | `g m` | everywhere | your mixes |
| `Search` | `/` | lists | filter the rows (see [Filtering a list](tui.md#filtering-a-list)) |
| `PreviousPage` | `backspace`, `C-q` | everywhere | back |
| `ClosePopup` | `esc` | popups, prompts, loads, filters | close / cancel (on a page: clear the focused window's filter) |
| `OpenCommandHelp` | `?`, `C-h` | everywhere | this help |
| `Quit` | `q`, `C-c` | everywhere | quit (an attached TUI detaches) |

`*` marks a command this player adds to spotify-player's.

Commands with parameters are written as an inline table: `command = { VolumeChange = { offset = -10 } }`, `command = { SeekBackward = { duration = 15 } }`, `command = { SeekForward = { } }` (the configured step).

### Actions

For `[[actions]]`; only `GoToRadio` has a default key (`r`, on the selected row).

| Action | Does | On |
|---|---|---|
| `GoToAlbum` | go to the album | tracks with an album |
| `GoToArtist` | go to the (first) artist | tracks, albums |
| `GoToRadio` | go to the radio (a page; nothing plays until `Enter`) | tracks, artists |
| `AddToQueue` | add to the end of the queue | tracks, albums, playlists |
| `PlayNext` | play next | tracks (not the playing one), albums, playlists |
| `AddToLiked` | add to favorites | tracks, albums, artists, playlists you follow |
| `DeleteFromLiked` | remove from favorites (asks `y/n` first) | tracks, albums, artists, playlists you follow |
| `AddToPlaylist` | add to a playlist… (opens the playlist picker) | tracks, albums |
| `DeleteFromPlaylist` | remove from this playlist (asks `y/n` first) | tracks on your own playlist's page |
| `RemoveFromQueue` | remove from the queue (asks `y/n` first) | queue entries, the playing track |
| `DeletePlaylist` | delete the playlist (still asks `y/n`) | your own playlists |

The three removals ask `y/n` the way the actions popup does, a bound key being no reason to skip it, unless `confirm_removals` is `false`; see [Confirming removals](tui.md#confirming-removals). *Delete playlist* asks whatever it says.

### spotify-player names not supported here

These are skipped, not errors, so a spotify-player `keymap.toml` can be copied as is. The TUI's message row says once at start which ones it skipped: `keymap.toml: 3 spotify-player commands not supported here: PlayRandom, LyricsPage, SortTrackByTitle`.

- Commands: `PlayRandom`, `RefreshPlayback`, `RestartIntegratedClient`, `ShowActionsOnCurrentContext`, `JumpToHighlightTrackInContext`, `JumpToCurrentTrackInContext`, `BrowseUserPlaylists`, `BrowseUserFollowedArtists`, `BrowseUserSavedAlbums`, `CurrentlyPlayingContextPage`, `TopTrackPage`, `RecentlyPlayedTrackPage`, `LyricsPage`, `BrowsePage`, `OpenSpotifyLinkFromClipboard`, `SortTrackByTitle`, `SortTrackByArtists`, `SortTrackByAlbum`, `SortTrackByDuration`, `SortTrackByAddedDate`, `ReverseTrackOrder`, `SortLibraryAlphabetically`, `SortLibraryByRecent`, `MovePlaylistItemUp`, `MovePlaylistItemDown`, `CreatePlaylist`, `OpenLogs`
- Actions: `GoToShow`, `AddToLibrary`, `DeleteFromLibrary`, `ShowActionsOnAlbum`, `ShowActionsOnArtist`, `ShowActionsOnShow`, `ToggleLiked`, `CopyLink`, `Follow`, `Unfollow`

A misspelt name (`NxtTrack`) is still an error.

## Errors

A broken `app.toml` or `keymap.toml` stops `tidal-player`, `tidal-player daemon` and `tidal-player play` with exit code 2 before anything starts (no socket, no terminal changes), with one line on stderr starting with the file's path:

| Problem | Message |
|---|---|
| TOML syntax | `…/app.toml: line 3, column 9: <parser message>` |
| Unknown setting (anywhere, `[layout]` included) | `…/app.toml: unknown setting "volum_step"` |
| Wrong type or range | `…/app.toml: invalid volume_step: expected an integer from 1 to 25, got 0` |
| Unknown field in `keymap.toml` | ``…/keymap.toml: line 2, column 1: unknown field `comand`, expected `command` or `key_sequence` `` |
| Unknown command or action | `…/keymap.toml: keymaps[4]: unknown command "NxtTrack"` (entries count from 0) |
| Bad key | `…/keymap.toml: keymaps[2]: unknown key "ctrl+s" in "ctrl+s"` |
| Bad parameter | `…/keymap.toml: keymaps[5]: VolumeChange offset must be from -25 to 25 and not 0, got 40` |
| A sequence of the file that starts another sequence of the file | `…/keymap.toml: "s" is bound to Search and is the start of "s l a" (LibraryPage); unbind those with command = "None" first` |
| Every `Quit` key unbound | `…/keymap.toml: Quit has no key left` |
| A file that cannot be read (permissions, a directory) | `…/app.toml: <error>` |

The `theme.toml` problems stop only `tidal-player` (the TUI, standalone or attached), the same way (exit code 2, one line on stderr, before the terminal changes); `tidal-player daemon` and `tidal-player play` do not read the file. Entries count from 0:

| Problem | Message |
|---|---|
| TOML syntax | `…/theme.toml: line 3, column 9: <parser message>` |
| Unknown field in a theme or its palette | `…/theme.toml: themes[1]: unknown field "colour"` / `…/theme.toml: themes[1].palette: unknown colour "orange"` |
| Unknown component | `…/theme.toml: themes[1].component_style: unknown style "blok_title"` |
| Unknown field in a style | `…/theme.toml: themes[1].component_style.border: unknown field "colour", expected fg, bg or modifiers` |
| Bad colour | `…/theme.toml: themes[1].palette.red: invalid colour "#ff55"` / `…/theme.toml: themes[1].component_style.border.fg: invalid colour "Purpel" (a palette name such as BrightBlack, or #rrggbb)` |
| Bad modifier | `…/theme.toml: themes[1].component_style.selection: unknown modifier "Blink"` |
| No name, an empty one, or one taken | `…/theme.toml: themes[3]: missing name` / `…: themes[3]: empty name` / `…: themes[3]: "dracula" is already themes[1]` |
| Unknown `theme` | `…/app.toml: unknown theme "draculla" (themes: default, dracula, gruvbox_dark)`; from the variable `TIDAL_PLAYER_THEME: unknown theme …`, from the flag `--theme: unknown theme …` |
| A file that cannot be read | `…/theme.toml: <io error>` |

A `theme` of the wrong type (`…/app.toml: invalid theme: expected a string, got 5`) stops every process, as any `app.toml` key does. A missing `theme.toml` is no error.

A file with a UTF-8 byte-order mark or Windows line ends reads fine.
