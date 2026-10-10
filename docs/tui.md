English | [繁體中文](zh-TW/tui.md)

# The TUI

Plain `tidal-player` (optionally with [items](playback.md#items)) opens the terminal interface. With no player running, the player and the screen run in one process (standalone): quitting stops playback and releases the audio device. With a player already running (a [daemon](daemon.md), or another TUI), the TUI attaches to it instead: the screen and the keys are the same, and quitting leaves the music playing (see [Attaching a TUI](daemon.md#attaching-a-tui)). Design: [spec 0004](specs/0004-queue-and-controls.md), [spec 0005](specs/0005-daemon-and-clients.md) and [spec 0006](specs/0006-library.md) (pages and the library) and [spec 0007](specs/0007-search.md) (search) and [spec 0011](specs/0011-mixes-and-radio.md) (mixes and radio) and [spec 0008](specs/0008-keymap-and-config.md) (keymap, config files and the keys help) and [spec 0013](specs/0013-key-hints.md) (key hints) and [spec 0014](specs/0014-device-selection.md) (the output device).

## The screen

One frame titled `tidal-player`, with the **playback window** (4 rows) at the top and one **page** below it. The first page is the **queue**; [Pages](#pages) lists the others:

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                     shuffle  repeat: queue  80%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: volume below 100% │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────  1:23 / 3:32│
│┌Queue (12)──────────────────────────────────────────────────────────────────┐│
││  1   May These Noises Start…  Pierce The Veil    Collide With The S…   4:01││
││▶ 2   Hell Above               Pierce The Veil    Collide With The S…   3:32││
││  3   A Match Into Water       Pierce The Veil    Collide With The S…   4:22││
│…                                                                             │
││  9   I'm Low On Gas And You…  Pierce The Veil    Collide With The S…   3:43││
││  ── Suggested ─────────────────────────────────────────────────────────────││
││  10  If I'm James Dean, You…  Sleeping With Si…  With Ears To See A…   3:39││
```

### Playback window

1. The state, the title and the artists, and on the right the modes:
   - `▶` playing, `⏸` paused, `…` loading or buffering, `■` stopped; `Nothing playing` when the queue is empty
   - `shuffle` when shuffle is on, `repeat: queue` or `repeat: track` when repeat is on, `autoplay` when autoplay is on, then the volume (`80%`), or `muted`
2. The album
3. How it plays: the quality Tidal granted, the format, the output device, and `bit-perfect` or why it is not (as the "Track" and "Output" lines of [`play`](playback.md#output-kinds-and-bit-perfect)), then ` · device released` while paused with the device [released](daemon.md#releasing-the-device-while-paused). When something went wrong, the message takes this row instead (see [Failures](#failures)); so does `Disconnected from the player: reconnecting…` while an attached TUI has lost its player
4. The progress bar, the position and the length. When Tidal does not give the length, there is no bar: `1:23 / ?:??`

At start the player resumes the last session: the queue it had, `■` stopped at the same position, with its modes and volume; `Space` plays from there (see [Resuming the last session](playback.md#resuming-the-last-session)).

### Queue

The queue in the order it plays (shuffled when shuffle is on). The playing track is marked `▶` and kept in view when it changes. The highlighted row is the **cursor**, which you move with the keys below; it stays on its track when the queue changes, and moves to the neighbouring row when its track is removed. Tracks added by [autoplay](#shuffle-repeat-and-autoplay) come after a `Suggested` row and are drawn dimmed.

In a narrow terminal the columns are cut with `…`; the album column goes first, then the artist. In a terminal of fewer than 8 rows (9 while the session-expired line shows) only the playback window is shown, whatever the page. When the session has expired, the last row says `Session expired — run "tidal-player login" in another terminal` (see [Logging in](login.md)).

## Keys

The default keys, with the command each one runs. Every key can be changed in `keymap.toml`, and the settings below in `app.toml`: see [Configuration](config.md), which lists every command with its key syntax (`C-s` is Control-s, `M-p` Alt-p, `backtab` Shift-Tab).

| Key | Command | Does |
|---|---|---|
| `space` | `ResumePause` | play/pause |
| `n` / `p` | `NextTrack` / `PreviousTrack` | next / previous track |
| `>` / `<` | `SeekForward` / `SeekBackward` | seek forward / backward by the seek step (5 s) |
| `^` | `SeekStart` | back to the start of the track |
| `C-s` | `Shuffle` | shuffle on/off |
| `C-r` | `Repeat` | repeat: off → queue → track → off |
| `A` | `ToggleAutoplay` | autoplay on/off |
| `+` / `-` | `VolumeUp` / `VolumeDown` | volume up / down by the volume step (5 %) |
| `_` | `Mute` | mute / unmute |
| `D` | `SwitchDevice` | choose the [output device](#output-device) (the Devices popup) |
| `o` | `AddToQueuePrompt` | add a link or track ID to the end of the queue |
| `O` | `PlayNextPrompt` | add a link or track ID to play next |
| `j`, `down`, `C-n` / `k`, `up`, `C-p` | `SelectNextOrScrollDown` / `SelectPreviousOrScrollUp` | move the cursor down, up |
| `g g` / `G`, `end` | `SelectFirstOrScrollToTop` / `SelectLastOrScrollToBottom` | move the cursor to the top, to the last loaded row |
| `C-f`, `page_down` / `C-b`, `page_up` | `PageSelectNextOrScrollDown` / `PageSelectPreviousOrScrollUp` | move the cursor down, up by a window's height |
| `enter` | `ChooseSelected` | on the queue: play the entry; on a page: play the track with its list, or open the album, playlist or artist (see [Playing and queueing](#playing-and-queueing-from-a-page)); in a popup: run the entry |
| `Z`, `C-z` | `AddSelectedItemToQueue` | add the selected track, album or playlist to the end of the queue |
| `d` | `RemoveFromQueue` | remove the entry under the cursor from the queue |
| `z` | `Queue` | open the queue page |
| `g l` | `LibraryPage` | open the library |
| `g y` | `LikedTrackPage` | open your favorite tracks |
| `g s` | `SearchPage` | open the [search](#search) page (on a search page: back to its input) |
| `g m` | `MixesPage` | open your [mixes](#mixes-and-radio) |
| `r` | `GoToRadio` (an `[[actions]]` entry) | open the [radio](#mixes-and-radio) of the selected track or artist |
| `/` | `Search` | on a search page: back to its input |
| `backspace`, `C-q` | `PreviousPage` | back to the previous page |
| `tab`, `backtab` | `FocusNextWindow`, `FocusPreviousWindow` | focus the next, previous pane of a page (on the artist page, the left or right half) |
| `[`, `]` | `PreviousTab`, `NextTab` | show the previous, next tab of the focused pane (the artist page's *Top tracks* / *All tracks* and *Albums* / *Appears on*) |
| `g a`, `C-space` | `ShowActionsOnSelectedItem` | the [actions](#actions) on the selected row |
| `a` | `ShowActionsOnCurrentTrack` | the actions on the playing track |
| `f` | `RoleFilter` | in an artist's *All tracks*: the [role filter](#the-role-filter) |
| `esc` | `ClosePopup` | close a popup or the open prompt, or cancel a list that is loading; does nothing otherwise |
| `?`, `C-h` | `OpenCommandHelp` | the [keys help](#the-keys-help) |
| `q`, `C-c` | `Quit` | quit (an attached TUI detaches; the player keeps playing) |

`g g` is two presses of `g`; `g l`, `g y`, `g s`, `g m` and `g a` are `g` and the second key. A `g` followed by any other key does what that key does. Wait after `g` and a [hint](#key-hints) lists the second keys. With an empty queue only the volume, mute and mode keys (and `o`/`O`, `q`) do something on the queue; the modes and the volume then apply to what you add next.

While a popup is open, only its own keys act (see [Actions](#actions)); `space`, `n`, `q` and the others do not reach the player. Text inputs (the `o`/`O` prompt, the search input, the playlist name) take every printable key whatever the keymap says.

The steps are `volume_step` (1–25 %, default 5) and `seek_duration_secs` (1–600 s, default 5) in `app.toml`, or `TIDAL_PLAYER_VOLUME_STEP` and `TIDAL_PLAYER_SEEK_STEP`; see [Settings](playback.md#settings). An attached TUI uses its own `keymap.toml` and steps, not the daemon's.

### The keys help

`?` (or `C-h`) opens a popup titled `Keys` anywhere except in a text input, listing the keys that act **where you are**: the open popup's or the focused window's keys first (`Queue`, `Library · Albums`, `Search · Tracks`, …), then `Lists`, `Pages`, `Playback`, `Actions` (your `[[actions]]` bindings) and `App`. The keys shown are the effective ones, after `keymap.toml`, in the file's syntax, so they can be pasted into it. A key that would do nothing right now (playback keys on an empty queue, anything while disconnected) is dimmed.

```
┌Keys────────────────────────────────────────────────────────┐
│ Library · Albums                                           │
│   enter           open the album                           │
│   Z  C-z          add to the end of the queue              │
│   g a  C-space    actions on the selected row              │
│ Lists                                                      │
│   j  down  C-n    move down                                │
│   …                                                        │
│ / filter · enter run · esc close                           │
└────────────────────────────────────────────────────────────┘
```

- The list keys (`j`, `k`, `C-f`, `G`, …) move the highlight over the keys
- `/` starts a **filter**: type to keep the keys whose keys, command name or text contain what you typed (ignoring case); `backspace` deletes, `enter` stops typing and keeps the filter, `esc` clears it. Nothing matches: `No keys match "xyz"`
- `enter` closes the help and **runs** the highlighted key, as if you had pressed it where you opened the help
- `esc` (with no filter), `?` or `q` close it. While it is open no other key acts

### Key hints

When you press the first key of a sequence (`g`) and wait, a **hint** box opens at the bottom of the page after a second, titled with the keys pressed so far (`g …`), listing the keys that can follow and what each does here:

```
│┌g …─────────────────────────────────────────────────────────────────────────┐│
││a  actions on the selected row     y  favorite tracks                       ││
││g  move to the top                 s  the search page (on one: its…         ││
││l  the library                     m  your mixes                            ││
│└────────────────────────────────────────────────────────────────────────────┘│
```

- Only keys that act where you are are listed, with the [keys help](#the-keys-help)'s texts and in its order; a key that would do nothing right now is dimmed. Your `keymap.toml`'s sequences are listed too, and a key that starts a longer sequence shows how many bindings it leads to (`l  +2`): press it to see the next level
- The hint changes nothing about the keys: press the next key whether it is shown or not. Typed faster than the delay, a sequence never shows it. It closes when the sequence completes, when a key starts nothing (`esc` cancels) or when the keys help opens
- When the entries do not fit, the last one reads `… +N more`; `?` lists them all. It is not drawn while the keys help is open, during a whole-list load, or in a terminal too small for the page
- `key_hints = false` in `app.toml` (or `TIDAL_PLAYER_KEY_HINTS=off`) turns it off; `key_hints_delay_ms` (0–10 000, default `1000`; `0` shows it at once, or `TIDAL_PLAYER_KEY_HINTS_DELAY_MS`) sets the wait. An attached TUI uses its own settings; see [`app.toml`](config.md#apptoml)

## Pages

The area below the playback window shows one page at a time. Opening a page puts it on top of a **history**; `Backspace` (or `Ctrl-q`) goes back to the page under it, exactly as you left it (its rows, cursors and focus), without fetching again. Opening a page always fetches it fresh. The TUI starts on the library, with the queue under it (`Backspace` or `z` shows it). The queue is always at the bottom of the history and cannot be closed; opening the page that is already on top does nothing, and the history keeps the last 50 pages.

| Page | Opened with | Windows (`Tab` moves between panes) |
|---|---|---|
| Queue | `z`; `Backspace` from the library at start | the queue |
| Library | `g l`; the page at start | Playlists, Albums, Artists |
| Favorite tracks | `g y` | the tracks |
| Search | `g s` | the input, the top hit, Tracks, Albums, Artists, Playlists (see [Search](#search)) |
| Mixes | `g m` | your mixes (see [Mixes and radio](#mixes-and-radio)) |
| Mix | `Enter` on a mix | the mix's tracks |
| Radio | `r` on a track or an artist; *Go to radio* | the radio's tracks |
| Album | `Enter` on an album; *Go to album* | the album's tracks |
| Playlist | `Enter` on a playlist | the playlist's tracks |
| Artist | `Enter` on an artist; *Go to artist* | two panes of two tabs: Top tracks \| All tracks, and Albums \| Appears on (`[` `]` switch a pane's tab) |

Each page has a title row above its windows: `Library`, `Favorite tracks · 362 tracks`, `<album> · <artists> · <year> · 17 tracks · 1:02:15`, `<playlist> · 39 tracks · 2:41:07`, `Mixes · 7 mixes`, `<mix> · 10 tracks`, `<track> Radio · <artists>`, `<artist> Radio`, or the artist's name. The counts are Tidal's totals, shown as soon as the first rows arrive.

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                     shuffle  repeat: queue  80%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: volume below 100% │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────  1:23 / 3:32│
│Library                                                                       │
│┌Playlists (22)───────────────┐┌Albums (14)──────────────────┐┌Artists (196)─┐│
││  Running                  42││Collide With Th…  Pierce The…││Pierce The Ve…││
││♥ Late night               17││Misadventures     Pierce The…││Sleeping With…││
││  Gym mix                   8││Hold On Till May  Pierce The…││Bring Me The… ││
│…                                                                             │
```

### Windows

A page with several windows draws them side by side when the frame is at least 60 columns wide inside:

- **Library**: Playlists 40 %, Albums 40 %, Artists 20 % (`[layout] library = { playlist_percent, album_percent }` in [`app.toml`](config.md#apptoml) changes the first two; Artists takes the rest)
- **Artist**: the left pane (60 %) has the tabs *Top tracks* and *All tracks*, the right pane (40 %) *Albums* (albums, then EPs and singles) and *Appears on*; a pane shows its active tab and its title lists the pane's tabs with the active one highlighted, then `[ ]`: `Top tracks (91) │ All tracks  [ ]` (a title too narrow for the other tab's name drops it)

Below 60 columns only the focused window is drawn, its title followed by `‹Tab›` (on the artist page the tab list and `[ ]` come first, then `‹Tab›` for the other pane). `Tab` focuses the next pane and `Shift-Tab` the previous, wrapping, each on the tab it last showed; `[` and `]` switch the focused pane's tab (*All tracks* is fetched the first time it shows); they do nothing on pages whose panes have one tab. Every window keeps its own cursor; the focused window's is highlighted and the others' are dimmed. Playlist rows show the number of tracks and a `♥` for playlists you follow; album rows the artists and the year (and `EP` or `Single`, when the window has room); tracks that Tidal does not stream in your country are dimmed, as the player will skip them.

### Lists load as you scroll

Every list is fetched a page at a time: the first page when the page opens, then the next when the cursor comes within one window height of the last loaded row. Meanwhile the last row says `Loading more…`; if that fails, the message takes its place, and the next cursor move near the end tries again. The total in a window's title is Tidal's, known from the start, so `Favorite tracks (362)` shows its size before the rows are all there. `G` goes to the last row loaded so far (and so loads the next page). A window's rows load while the music plays and never delay a key.

The page size is `page_size` in `app.toml` or `TIDAL_PLAYER_PAGE_SIZE` (1–10 000, default 100; the playlists and credits lists are fetched in pages of at most 50); see [Settings](playback.md#settings).

While a page is being fetched its windows say `Loading…`. A failed fetch shows its message in the page (`Could not load the library: Could not reach Tidal: …`, `Album 123 was not found`, the session-expired message); `Backspace` goes back, and opening the page again tries again. An empty list says so: `No playlists yet`, `No favorite albums yet`, `No favorite artists yet`, `No favorite tracks yet`, `This album has no tracks`, `This playlist has no tracks`, `No top tracks`, `No albums`, `No credits` (`No credits (37 hidden)` when the filters left nothing).

While the player is unreachable (an attached TUI whose player went away) the loaded pages can still be browsed and the history used, but nothing new is fetched: a page opened then shows `Disconnected from the player: reconnecting…` and is fetched again when the player is back.

### The artist's *All tracks*

*All tracks* is Tidal's "Credits for <artist>": every track the artist is credited on, as performer, songwriter, producer or engineer, most popular first. It is fetched when you first focus it. Wide windows show the artist's role categories on each track (`Performer, Songwriter`) next to the album.

Left out, with the number hidden in the window's title (`All tracks (548 · 37 hidden)`): copies without a stereo mix (Dolby Atmos only, which the player cannot decode) and **alternate versions**: a track whose version, or a bracketed part or ` - …` suffix of its title, is one of the hidden words, such as `Instrumental`, `TV Size`, `Sped Up` or `Slowed + Reverb`. `Acoustic`, `Live` and remixes stay. The words are `TIDAL_PLAYER_HIDE_VERSIONS`; see [Settings](playback.md#settings). The other windows show every version.

#### The role filter

`f` in *All tracks* opens a popup with the four role categories (Performer, Songwriter, Producer, Engineer) as check boxes, all checked at first. `j`/`k` move, `Space` toggles one, `Enter` applies, `Esc` cancels. Only tracks where the artist has a checked role are shown, and the title says which are (`All tracks (548 · Performer, Songwriter · 37 hidden)`). More rows load as usual while you scroll. The filter lasts as long as the page is in the history.

## Search

`g s` opens the search page: a one-row input (`Search: `) with the cursor in it, a **top hit** row, and four windows, *Tracks* and *Albums* over *Artists* and *Playlists*.

```
│Search · "pierce the veil"                                                    │
│Search: pierce the veil                                                       │
│Top hit: Pierce The Veil · artist                                             │
│┌Tracks (223)──────────────────────┐┌Albums (55)──────────────────────────────┐│
││King For A Day    Pierce The Veil ││Collide With The Sky  Pierce The Veil 2012││
││Hell Above        Pierce The Veil ││The Jaws Of Life      Pierce The Veil 2023││
│┌Artists (7)───────────────────────┐┌Playlists (3)────────────────────────────┐│
││Pierce The Veil                   ││Pierce The Veil Essentials          15   ││
│…                                                                             │
```

**Typing.** While the input has the cursor every key types into it, so `q`, `n`, `Space` and `g` do not reach the player; paste works too. `Backspace` deletes the last character (on an empty input it does nothing), `Ctrl-u` clears the input, and a query stops at 200 characters. `Ctrl-c` still quits and `Ctrl-q` still goes back.

**Searching.** `Enter` searches (an empty input does nothing). The windows say `Loading…`, then show Tidal's results with their totals, and the cursor moves to the top hit (else the first window with results). A new search on the same page replaces the results. Tidal's **top hit** is its best match, of any kind: `Pierce The Veil · artist`, `Collide With The Sky · album`, a track or a playlist; there is no top-hit row when Tidal names none.

**Moving around.** `Tab` and `Shift-Tab` go input → top hit → Tracks → Albums → Artists → Playlists → input. On a window every key works as on any page (cursor keys, `Enter`, `Z`, `g a`, `Backspace`, …); `/` goes back to the input, and so does `g s`. `Esc` in the input moves to the results. Going back to a search page through the history shows its query and results as you left them; `g s` from another page opens a new, empty search page.

**Results load as you scroll**, 20 at a time (`TIDAL_PLAYER_SEARCH_PAGE_SIZE`, 1–1000; see [Settings](playback.md#settings)), until the window holds all of Tidal's results (Tidal has a few hundred at most for a query). Tracks that exist only in Dolby Atmos are left out, as the player cannot play them, so a title's total can be a little higher than its rows.

**Playing.** `Enter` on a result track plays it with the tracks **already loaded** in *Tracks* queued around it, in result order: right after a search that is 20 tracks; scroll further first to queue more. Nothing more is fetched for that queue. `Enter` on the top hit does the same for a track (a top-hit track that is not among the loaded rows plays first, the loaded rows after it) and opens the page of an album, artist or playlist. Albums, artists and playlists open their pages; `Z` and the [actions](#actions) work as on other pages (search playlists are never your own).

Nothing matches: `No tracks found`, `No albums found`, `No artists found`, `No playlists found`. A failed search shows `Could not search: <reason>` in the page.

## Mixes and radio

`g m` opens your **mixes**: the daily mixes, *My Daily Discovery* and the others Tidal builds for you, in Tidal's order, each row its title and then its subtitle (`My Mix 1  Pierce The Veil, Sleeping With Sirens and more`), cut with `…` when it does not fit. `Enter` on a mix opens its page: the mix's title in the title row, its subtitle under it in dim (when the page is at least 8 rows tall), and its tracks. A mix is a list of tracks like an album's: `Enter` on a track plays it with the rest of the mix queued; `Z` on a mix does nothing (open it first), and its actions popup has *Open* only.

`r` on a selected track or artist (on any page, the queue included) opens its **radio**: `<track> Radio · <artists>` or `<artist> Radio`, up to 100 tracks Tidal suggests. Nothing plays and the queue stays as it is until you press `Enter` on a track (it plays with the rest of the radio queued) or `Z`. *Go to radio* in the [actions](#actions) popup does the same, and so does an `[[actions]]` entry bound to another key (see [config](config.md#actions)); albums, playlists and mixes have no radio.

The pages arrive whole, so they have no scroll loading, and each is fetched fresh when opened; `Backspace` goes back to a page as you left it. Messages in the page: `No mixes yet`, `This mix has no tracks`, `No radio for this track`, `No radio for this artist` (Tidal has none for it, not a failure), and for failures `Could not load the mixes: <reason>`, `Mix <id> was not found`, `Track 1 was not found`, `Artist 1 was not found`. Track rows are the usual ones, a track you cannot stream dimmed.

## Playing and queueing from a page

| Key | On | Does |
|---|---|---|
| `Enter` | a track on a page | Replaces the queue with **every track of that list**, in the order shown, and plays the one you chose, so the queue goes on after it |
| `Enter` | a track in the queue | Plays that entry |
| `Enter` | an album, playlist, artist or mix | Opens its page (nothing plays) |
| `Z`, `Ctrl-z` | a track | Adds it to the end of the queue; with nothing playing it starts |
| `Z`, `Ctrl-z` | an album or playlist | Adds all its tracks to the end of the queue |
| `Z`, `Ctrl-z` | an artist or a mix | Nothing |
| `d` | an entry in the queue | Removes it (removing the playing one moves on) |

`Enter` on a track needs the whole list, so a list that is only partly loaded is first fetched to its end: the playback window's message row says `Loading 300 of 1 234…`, and `Esc` cancels it with nothing sent. A list of more than 40 000 tracks is refused (`Too many tracks to queue at once (N)`).

## Actions

`g a` or `Ctrl-Space` opens a small popup of actions on the selected row, titled with its name; `a` opens it for the playing track (nothing happens when nothing plays). `j`/`k` move, `Enter` runs the action and closes the popup, `Esc` closes it.

| On | Actions, in this order |
|---|---|
| A track on a page | *Go to album*, *Go to artist: <name>* (one per artist), *Go to radio*, *Add to queue*, *Play next*, *Add to favorites*, *Remove from favorites*, *Add to playlist…*, and on a page of your own playlist *Remove from this playlist* |
| A queue entry, or the playing track | *Go to album*, *Go to artist: …*, *Go to radio*, *Play next* (not for the playing track), *Remove from queue*, *Add to favorites*, *Remove from favorites*, *Add to playlist…* |
| An album | *Open*, *Go to artist: …*, *Add to queue*, *Play next*, *Add to favorites*, *Remove from favorites*, *Add to playlist…* |
| A playlist | *Open*, *Add to queue*, *Play next*, *Add to favorites*, *Remove from favorites* (not your own playlists), *Delete playlist* (your own only) |
| An artist | *Open*, *Go to radio*, *Add to favorites*, *Remove from favorites* |
| A mix | *Open* |

*Go to album* is left out for a track without an album. Tidal's favorites cannot be queried for one item yet, so both *Add to favorites* and *Remove from favorites* are offered; the one that does not apply is harmless.

### Favorites and playlists

The result shows in the playback window's message row: `Added to favorites`, `Removed from favorites`, `Added 12 tracks to <playlist>`, `Removed from <playlist>`, `Created <playlist>`, `Deleted <playlist>`, or the error. A page that shows what changed (the favorites, the library, the playlist) is fetched again after a change.

- **Add to playlist…** opens a second popup listing your own playlists (newest first) under *New playlist…*; `Enter` adds the track, or all the tracks of the album, at the end of the playlist. If a track is already in the playlist (as far as the loaded copy shows) it asks `Already in <playlist>: add again? (y/n)`
- **New playlist…** asks `Playlist name: ` in the top row of the page; `Enter` creates a private playlist with that name and adds the tracks to it; `Esc` cancels; an empty name does nothing
- **Remove from this playlist** removes the track at that position. If the playlist was changed elsewhere in the meantime, nothing is removed (`The playlist changed: nothing was changed, try again`) and the page is fetched again
- **Delete playlist** asks `Delete <playlist>? (y/n)`; `y` deletes it, `n` or `Esc` cancels

## Adding tracks

`o` opens a prompt over the top of the queue, `Add to queue: `; `O` opens `Play next: `. Type or paste (the terminal's paste, `Ctrl-Shift-v` in most terminals) a track ID or a Tidal track, album or playlist link, as on the [command line](playback.md#items), then:

- `Enter` sends the item to the player, which fetches the tracks and adds them at the end of the queue (`o`) or right after the playing track (`O`). If nothing is playing (an empty queue, or a stopped player with no current track), the first added track starts. The queue shows them once the player has added them
- `Backspace` deletes the last character; `Esc` closes the prompt without adding anything (`Esc` otherwise does nothing: `q` quits)

While the prompt is open every key types into it: `Space`, `q` and the other keys do not reach the player. An invalid item (`Not a Tidal track, album or playlist: …`) or a failed fetch (`Album 123 was not found`) closes the prompt and shows the message in the playback window; the queue is unchanged.

## Playback

- **Previous** (`p`) goes back to the start of the track when more than 3 seconds have played, otherwise to the previous track (`TIDAL_PLAYER_PREVIOUS_RESTART`, 0–60 s; `0` makes previous always go back). On the first track it goes back to the start
- **Next** (`n`) at the end of the queue (and repeat off, autoplay off) stops on the last track at 0:00; `Space` then plays it again
- **Seeking** past the end ends the track, which then moves on as it would at its natural end; before 0:00 it stops at 0:00. Seeking while a track loads sets where it starts
- **Pause while loading**: the track keeps loading but starts only when you press `Space` again
- Tracks follow each other without a gap when their formats match (the next track is prepared 30 seconds before the end)

### Shuffle, repeat and autoplay

- **Shuffle** (`Ctrl-s`) puts the playing track first and shuffles the rest; the queue shows the new order. Turning it off restores the original order. The playing track keeps playing either way. Tracks added while shuffle is on go where you add them; they are not shuffled in
- **Repeat** (`Ctrl-r`): `queue` starts over at the first track after the last one; `track` plays the same track again at its end (`n` still moves on)
- **Autoplay** (`A`, off by default; `TIDAL_PLAYER_AUTOPLAY=on` turns it on at start) keeps the music going when the queue runs out: near the end of the last track, Tidal is asked for tracks related to it, and they are added to the queue (under `Suggested`, without tracks already queued) and play on without a gap. Repeat `queue` or `track` takes precedence. When Tidal has nothing to suggest, the queue stops at its end with `Autoplay: no suggestions (…)`

### Volume

From 0 to 100 %, **100 % by default**, in steps of the volume step. At 100 % the samples are not touched, so a bit-perfect output stays bit-perfect; below 100 % (or muted) the third row says `not bit-perfect: volume below 100%` (or `muted`). The volume follows a curve that matches how loudness is heard (50 % is about −18 dB). Mute (`_`) keeps the volume, so unmuting restores it; changing the volume unmutes. The volume and mute are remembered across runs, with the queue and the modes (see [Resuming the last session](playback.md#resuming-the-last-session)).

## Output device

`D` opens the **Devices** popup from any page: the player's output devices, read from the player's machine each time it opens (a DAC plugged in since shows up), with `●` on the device the player uses now:

```
┌Devices─────────────────────────────────────────┐
│● default   shared, through the system mixer    │
│  hw:0,0    HDA Intel PCH: ALC892 Analog        │
│  hw:1,0    E30 II: USB Audio                   │
└────────────────────────────────────────────────┘
```

| Key | Does |
|---|---|
| `j`, `k` (and the arrow keys) | move the cursor (it starts on the device in use) |
| `Enter` | switch to that device and close the popup; on the device in use it just closes |
| `r` | read the list again |
| `esc`, `q` | close without changing anything (`q` does not quit here) |

- `Loading devices…` shows until the list arrives; `Cannot list devices: <reason>` when it cannot be read. While an attached TUI is disconnected the popup says so, and `D` is dimmed in the [keys help](#the-keys-help)
- A device in use that the list does not have (`plughw:1,0`, a custom PCM, an unplugged DAC) comes first, marked `●`, as `not found`
- **Playing**, the track moves to the new device at its position, with nothing lost or played twice; the third row shows the new output (`hw:1,0 (exclusive) …, bit-perfect`) as soon as it is open. **Paused**, the new device is opened paused: nothing plays until `space`. **Stopped**, loading or with the device [released](daemon.md#releasing-the-device-while-paused), nothing is opened: the next play opens the new device
- If the new device cannot be opened (busy, missing, refused), the player goes back to the device it was using and carries on there, with the message `Cannot switch to hw:1,0: Output hw:1,0 is busy (used by …): …; staying on default`. Nothing is skipped and the queue is unchanged
- The choice is the **player's**, so every attached TUI and `tidal-player playback status` show it; `tidal-player playback device` lists and switches from a shell (see [One-shot commands](daemon.md#one-shot-commands))
- It lasts for the run: a restarted player starts on the configured device again (`output_device` in `app.toml`, `--device` or `TIDAL_PLAYER_DEVICE`; see [Settings](playback.md#settings)). Put the device there to keep it

## Failures

The message shows in the playback window, in place of the format row, until the next track starts or another message replaces it.

| What failed | Examples | What happens |
|---|---|---|
| The track only | not available in your country, preview only, not found, not decodable | The message shows and the next track plays. After 5 such tracks in a row (or the whole queue, if shorter) playback stops: `Stopped: 5 tracks in a row could not be played` |
| The network or Tidal | the connection dropped after retries, `429`, `5xx` | Stops on that track, where it was. Never skips. `Space` tries again |
| The output | `Output hw:1,0 is busy …`, `No such output device …`, `Output hw:1,0 was lost` | Stops on that track; the queue is kept. `Space` tries again on the same device |
| The session | Tidal no longer accepts the login | Stops; the last row shows the session-expired status. After `tidal-player login` in another terminal, `Space` resumes |

The messages are the same as for `play`: see [Errors](playback.md#errors).
