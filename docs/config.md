English | [繁體中文](zh-TW/config.md)

# Configuration

Two optional files configure tidal-player: **`app.toml`** for the settings and **`keymap.toml`** for the keys, in [spotify-player](https://github.com/aome510/spotify-player)'s format so its `[[keymaps]]` blocks can be copied across. Design: [spec 0008](specs/0008-keymap-and-config.md).

## Where the files are

The **config directory** is, first found:

1. `-c DIR` / `--config-folder DIR`
2. `$TIDAL_PLAYER_CONFIG_DIR`
3. `$XDG_CONFIG_HOME/tidal-player`
4. `~/.config/tidal-player`

An empty variable counts as unset. The flag goes **after** the subcommand: `tidal-player daemon -c DIR`, `tidal-player play -c DIR 123`, and plain `tidal-player -c DIR` for the TUI.

The directory and both files may be missing: the defaults are used, and nothing is ever created or written there. The files are read **once, at start**; a daemon picks up a changed file when restarted (`systemctl --user restart tidal-player`).

**Starting points**: [`examples/app.toml`](../examples/app.toml) and [`examples/keymap.toml`](../examples/keymap.toml) spell out every default (each setting with its range, each key binding with its command). Copied as they are they change nothing; edit what you want and delete the rest:

```sh
mkdir -p ~/.config/tidal-player
cp examples/app.toml examples/keymap.toml ~/.config/tidal-player/
```

Each process uses what concerns it: the player (the standalone TUI, the daemon, `play`) the player settings, the TUI (standalone or attached to a daemon) the TUI settings and `keymap.toml`. An attached TUI uses its own `keymap.toml` and TUI settings, not the daemon's. Still, every process that starts a player or a TUI checks **both** files whole: a typo in a key stops the daemon too, so one restart shows it.

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
| `key_hints` | `true`, `false` | `true` | `TIDAL_PLAYER_KEY_HINTS` (`on`, `off`) | TUI |
| `key_hints_delay_ms` | integer 0–10 000 (ms; `0`: at once) | `1000` | `TIDAL_PLAYER_KEY_HINTS_DELAY_MS` | TUI |
| `page_size` | integer 1–10 000 | `100` | `TIDAL_PLAYER_PAGE_SIZE` | player, TUI |
| `search_page_size` | integer 1–1000 | `20` | `TIDAL_PLAYER_SEARCH_PAGE_SIZE` | player, TUI |
| `hide_versions` | array of strings (`[]` hides nothing) | the sixteen words of [Settings](playback.md#settings) | `TIDAL_PLAYER_HIDE_VERSIONS` | player |
| `[layout] library = { playlist_percent, album_percent }` | integers 1–98 each, sum at most 99; *Artists* takes the rest | `40`, `40` | | TUI |

What each setting does is described in [Settings](playback.md#settings); the library layout in [Windows](tui.md#windows); `key_hints` and `key_hints_delay_ms` in [Key hints](tui.md#key-hints); `mpris` and `max_cover_arts` in [Desktop controls and media keys](mpris.md). The album covers are kept in the **cache directory**: `$TIDAL_PLAYER_CACHE_DIR`, else `$XDG_CACHE_HOME/tidal-player`, else `~/.cache/tidal-player` (see [Album covers](mpris.md#album-covers)).

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

Pressed keys collect: when what you pressed is no binding's start, collecting restarts with the last key alone (`g x` does what `x` does); when it is a binding, that binding runs. There is no timeout. A sequence that is the start of another (`g` bound while `g g` is) is refused, see [Errors](#errors).

**Unreliable keys**: many terminals cannot send `C-enter`, `C-tab`, `C-backspace` or Control with the arrows as distinct keys; such a binding is accepted but may never fire. A terminal sends `C-i` as `tab`, `C-m` as `enter` and `C-[` as `esc`, so bind those names instead. `M-` keys need a terminal that sends Alt as Escape + key (most do; on macOS, "Use Option as Meta"). The keys help (`?`) shows what is bound; pressing the key shows whether it arrives.

### Where keys act

A binding acts only where its command does (the "Acts in" column below); elsewhere it does nothing. In a popup only its own keys act: the list commands, `ChooseSelected`, `ClosePopup` and `OpenCommandHelp`.

Some keys are **fixed**, not in the keymap: text inputs (the `o`/`O` prompt, the search input, the playlist-name prompt) take every printable character and paste before the keymap, with their editing keys `backspace`, `C-u`, `enter`, `esc` (in the search input also `tab`/`backtab`, `C-c` quits and `C-q` goes back); the role filter's `space`; the questions' `y` and `n`; the keys help's `/` filter.

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
| `RemoveFromQueue` | `d` | the queue | remove from the queue |
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
| `Search` | `/` | the search page | back to the search input |
| `PreviousPage` | `backspace`, `C-q` | everywhere | back |
| `ClosePopup` | `esc` | popups, prompts, loads | close / cancel |
| `OpenCommandHelp` | `?`, `C-h` | everywhere | this help |
| `Quit` | `q`, `C-c` | everywhere | quit (an attached TUI detaches) |

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
| `DeleteFromLiked` | remove from favorites | tracks, albums, artists, playlists you follow |
| `AddToPlaylist` | add to a playlist… (opens the playlist picker) | tracks, albums |
| `DeleteFromPlaylist` | remove from this playlist | tracks on your own playlist's page |
| `RemoveFromQueue` | remove from the queue | queue entries, the playing track |
| `DeletePlaylist` | delete the playlist (still asks `y/n`) | your own playlists |

### spotify-player names not supported here

These are skipped, not errors, so a spotify-player `keymap.toml` can be copied as is. The TUI's message row says once at start which ones it skipped: `keymap.toml: 3 spotify-player commands not supported here: PlayRandom, LyricsPage, SwitchTheme`.

- Commands: `PlayRandom`, `RefreshPlayback`, `RestartIntegratedClient`, `SwitchTheme`, `ShowActionsOnCurrentContext`, `JumpToHighlightTrackInContext`, `JumpToCurrentTrackInContext`, `BrowseUserPlaylists`, `BrowseUserFollowedArtists`, `BrowseUserSavedAlbums`, `CurrentlyPlayingContextPage`, `TopTrackPage`, `RecentlyPlayedTrackPage`, `LyricsPage`, `BrowsePage`, `OpenSpotifyLinkFromClipboard`, `SortTrackByTitle`, `SortTrackByArtists`, `SortTrackByAlbum`, `SortTrackByDuration`, `SortTrackByAddedDate`, `ReverseTrackOrder`, `SortLibraryAlphabetically`, `SortLibraryByRecent`, `MovePlaylistItemUp`, `MovePlaylistItemDown`, `CreatePlaylist`, `OpenLogs`
- Actions: `GoToShow`, `AddToLibrary`, `DeleteFromLibrary`, `ShowActionsOnAlbum`, `ShowActionsOnArtist`, `ShowActionsOnShow`, `ToggleLiked`, `CopyLink`, `Follow`, `Unfollow`

A misspelt name (`NxtTrack`) is still an error.

## Errors

A broken file stops `tidal-player`, `tidal-player daemon` and `tidal-player play` with exit code 2 before anything starts (no socket, no terminal changes), with one line on stderr starting with the file's path:

| Problem | Message |
|---|---|
| TOML syntax | `…/app.toml: line 3, column 9: <parser message>` |
| Unknown setting (anywhere, `[layout]` included) | `…/app.toml: unknown setting "volum_step"` |
| Wrong type or range | `…/app.toml: invalid volume_step: expected an integer from 1 to 25, got 0` |
| Unknown field in `keymap.toml` | ``…/keymap.toml: line 2, column 1: unknown field `comand`, expected `command` or `key_sequence` `` |
| Unknown command or action | `…/keymap.toml: keymaps[4]: unknown command "NxtTrack"` (entries count from 0) |
| Bad key | `…/keymap.toml: keymaps[2]: unknown key "ctrl+s" in "ctrl+s"` |
| Bad parameter | `…/keymap.toml: keymaps[5]: VolumeChange offset must be from -25 to 25 and not 0, got 40` |
| A sequence that starts another | `…/keymap.toml: "g" is bound to LibraryPage and is the start of "g g" (SelectFirstOrScrollToTop), …; unbind those with command = "None" first` |
| Every `Quit` key unbound | `…/keymap.toml: Quit has no key left` |
| A file that cannot be read (permissions, a directory) | `…/app.toml: <error>` |

A file with a UTF-8 byte-order mark or Windows line ends reads fine.
