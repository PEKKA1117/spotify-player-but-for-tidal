# 0010 — MPRIS2 and media keys

- **Status**: approved (2026-10-09)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0002/0006 (implemented: the metadata and library clients that parse tracks), 0004 (implemented: the player and its protocol), 0005 (implemented: one player per user, the hub of clients, one-shot commands), 0008 (implemented: `app.toml` and its precedence), 0009 (implemented: a restored player is stopped)
- **User docs**: a new [`docs/mpris.md`](../mpris.md) (desktop controls, media keys, `playerctl`); [`docs/daemon.md`](../daemon.md) (the new one-shot commands, "`playerctl` works"), [`docs/config.md`](../config.md) (`mpris`, `max_cover_arts`, the cache directory), [`docs/playback.md`](../playback.md) (link to the new page) and their zh-TW copies (AC17)

## Context

The player can only be controlled from its own TUI and from `tidal-player playback …`. Desktop shells (GNOME, KDE Plasma), bars (waybar, polybar), lock screens, Bluetooth headsets (through BlueZ's AVRCP bridge), KDE Connect and `playerctl` all control players through **MPRIS2**, the D-Bus interface `org.mpris.MediaPlayer2`; the keyboard's media keys reach a player the same way on every current Linux desktop (`gsd-media-keys`, KWin, and the `playerctl` bindings of tiling window managers). 0001 planned it, 0004 deferred "setter" commands to it, and 0005 decided that MPRIS is a **public adapter on top of the player**, separate from the client socket (0005 decision 1).

This spec makes **every player process** (standalone TUI, `daemon`, `play`) publish itself on the session bus as an MPRIS2 media player, reflect its state there live, and act on MPRIS calls through the same `Command`s the clients send.

### What tidalt did

Read from tidalt's `internal/mpris/server.go`, `internal/ui/model.go` (`SetState`) and commits `3bace90`, `c7d5e2d`, `9660ad1`, `c914a5d`:

- `org.mpris.MediaPlayer2.tidalt` at `/org/mpris/MediaPlayer2`, with `org.mpris.MediaPlayer2`, `org.mpris.MediaPlayer2.Player`, a hand-written `org.freedesktop.DBus.Properties` and `Introspectable`, plus its private `io.tidalt.App` on the same object. Claiming the name doubled as the "one instance" lock (0005 "What tidalt did")
- The UI model pushed a `PlayerState` (JSON strings of the track and playlist) into a mutex-guarded copy; `Get`/`GetAll` read it on demand
- `Player` implemented `PlayPause`, `Next`, `Previous`; **`Play` and `Pause` both sent the play/pause toggle**; `Stop` did nothing
- Calls became events on a 4-slot channel with a **non-blocking send**: a busy UI dropped the call and the caller still got success
- Properties: `PlaybackStatus`, `Metadata`, `CanPlay`/`CanPause`/`CanGoNext`/`CanGoPrevious` always `true`, `CanSeek` `false`, `Rate` 1.0. **Not implemented**: `Position`, `Seek`, `SetPosition`, `Seeked`, `Volume`, `Shuffle`, `LoopStatus`, `OpenUri`
- `Metadata`: `mpris:trackid` `/org/mpris/MediaPlayer2/Track/<tidal track id>`, `xesam:title`, `xesam:artist` (the **first artist only**), `xesam:album`, `mpris:length`
- `Set` answered `PropertyReadOnly` for everything, and **`PropertiesChanged` was never emitted**
- No `mpris:artUrl`, although the TUI drew the album cover (Kitty graphics, later sixel: `internal/ui/coverart.go`, `becf508`) from `tidal.CoverURL(track.Album.Cover, "640x640")`: `https://resources.tidal.com/images/<cover UUID with each - replaced by />/<size>.jpg`

### What went wrong there, and the criterion that covers it here

1. **No `PropertiesChanged`**: GNOME's media widget, waybar and `playerctl --follow` only update on that signal, so they showed the track the player had when they first looked → every change of a `Player` property except `Position` emits one `PropertiesChanged` with exactly the changed properties, after the state change and in order (AC5)
2. **`Play`/`Pause` toggled**: `playerctl pause` on a paused player resumed it, and a headset's pause button did the same → `Play`, `Pause`, `Stop` are explicit player commands, decided in the player against its real state, never a toggle computed from a stale copy (AC1, AC8)
3. **Calls were dropped silently** (the non-blocking send) → each call reaches the player through the client hub like any client's request, is answered after the player handled it, and a call the player refuses or does not answer in time returns a D-Bus error (AC9)
4. **Capabilities were lies** (`CanGoNext` with an empty queue, `CanSeek` false while seek existed in the TUI) → the `Can*` properties are derived from the state, and change with it (AC3)
5. **Track IDs from Tidal IDs** (a track queued twice had one ID, and `/org/mpris` is reserved by the MPRIS spec for its own paths) → `mpris:trackid` is made from the **queue entry ID**, under our own path (AC2)
6. **One artist, no cover**: `xesam:artist` lists every artist of the track, and `mpris:artUrl` gives the album cover (AC4, AC18)
7. **The bus name was the single-instance lock**: no session bus meant no player at all, and a stale name blocked a start → the player's lock stays 0005's file lock; MPRIS is optional: no session bus, or a name already taken, never stops a player (AC11, AC12)

## Behaviour

### Where it runs

Every player process (the standalone TUI, `tidal-player daemon`, `tidal-player play`) connects to the **session bus** at start, after the restored state is loaded (0009) and before it accepts clients, and owns:

- the bus name `org.mpris.MediaPlayer2.tidal_player` (`playerctl -p tidal_player`), requested with `DoNotQueue`. If the name is taken (a player run with another `TIDAL_PLAYER_RUNTIME_DIR`), it takes `org.mpris.MediaPlayer2.tidal_player.instance<pid>`, as the MPRIS spec prescribes for second instances
- the object `/org/mpris/MediaPlayer2` with `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` (and the standard `Properties`, `Introspectable`, `Peer`)

An attached TUI and a one-shot command never touch the bus (they are clients, 0005). When the player exits, the name is released (its bus connection closes) after `ShuttingDown` reached the clients.

No session bus (`DBUS_SESSION_BUS_ADDRESS` unset and no `$XDG_RUNTIME_DIR/bus`, a headless box over SSH), a refused connection, or a failure to export the object: the player runs as before without MPRIS and logs one line (`MPRIS is not available: <reason>`) at `info`. It is **not** the player's message: a box without a desktop is normal. The bus is never retried during the run.

### Turning it off

`mpris` in `app.toml` (`true` by default), or `TIDAL_PLAYER_MPRIS` (`on`/`off`), with 0008's precedence. Off: the player never connects to the session bus for MPRIS (0003's device reservation is unaffected).

### `org.mpris.MediaPlayer2`

| Property / method | Value |
|---|---|
| `Identity` | `tidal-player` |
| `CanQuit` | `false`; `Quit()` does nothing (a desktop widget must not stop a daemon; `daemon stop` and `q` do) |
| `CanRaise` | `false`; `Raise()` does nothing |
| `HasTrackList` | `false` |
| `SupportedUriSchemes` | `["tidal", "https"]` |
| `SupportedMimeTypes` | `[]` |
| `DesktopEntry`, `Fullscreen`, `CanSetFullscreen` | not provided (no `.desktop` file is installed) |

### `org.mpris.MediaPlayer2.Player`: properties

From the player's state (the snapshot every client gets, 0004):

| Property | Value |
|---|---|
| `PlaybackStatus` | `Playing` for `Playing`, `Buffering`, and `Loading` that is not held by a pause; `Paused` for `Paused` (released or not, 0005) and a held load; `Stopped` for `Stopped` |
| `LoopStatus` (rw) | `None`, `Playlist`, `Track` for repeat `off`, `queue`, `track` |
| `Shuffle` (rw) | shuffle |
| `Volume` (rw) | volume / 100 (`0.8` for 80 %); `0.0` while muted |
| `Position` | the current entry's position in µs: the last known position, plus the time since it was reported while `Playing` (capped at the track's duration); `0` with no current entry |
| `Rate`, `MinimumRate`, `MaximumRate` (rw) | `1.0`; setting `Rate` does nothing |
| `Metadata` | below |
| `CanGoNext` | there is a current entry and `Next` would make another entry current: an entry after it in play order, or repeat `queue`/`track` (0004: `Next` wraps when repeat is on) |
| `CanGoPrevious` | there is a current entry (`Previous` goes back or restarts, 0004) |
| `CanPlay` | there is a current entry |
| `CanPause` | there is a current entry |
| `CanSeek` | there is a current entry and its duration is known |
| `CanControl` | `true` (constant, as the MPRIS spec requires) |

**`Metadata`** of the current entry; with no current entry, only `mpris:trackid` `/org/mpris/MediaPlayer2/TrackList/NoTrack`:

| Key | Value |
|---|---|
| `mpris:trackid` | `/tidal_player/entry/<entry id>` (an object path; unique per queue entry, so a track queued twice has two) |
| `mpris:length` | the duration in µs (from the stream once started, 0004), omitted when unknown |
| `xesam:title` | the title, with ` (<version>)` when the track has a version (as the TUI shows it, 0006) |
| `xesam:artist` | every artist, in Tidal's order |
| `xesam:album` | the album title, omitted when none |
| `xesam:url` | `https://tidal.com/browse/track/<track id>` |
| `mpris:artUrl` | the album cover: `file://<cache dir>/covers/<cover>.jpg` once it is in the cache; Tidal's `https://` URL when the cache is off or the download failed; omitted while it downloads, and when the track has no album or Tidal gave no cover (see "Cover art") |
| `xesam:trackNumber`, `xesam:albumArtist` | not provided (not in the queue's `Track`) |

### Cover art

Tidal returns the album's cover image ID with every track (`album.cover`, a UUID, in the v1 track objects of `/tracks`, album, playlist, favorites, search and mix pages). This spec carries it through:

- `AlbumRef` gains `cover: Option<String>`, filled by the metadata and library clients wherever they build a track (0002's `MetadataClient`, 0006's `LibraryClient`); a missing or `null` cover is `None`
- It travels with the queue (`PlayerSnapshot`, 0004) and is remembered in `playback.json` (0009): a file written before this spec, without the field, still loads (the field defaults to none; no `version` change), and its entries get no cover until they are queued again
- The image URL is built in one pure function (`tidal_player_core::track::cover_url(cover, size)`): `https://resources.tidal.com/images/<cover with each - replaced by />/<size>.jpg`. The player uses `640x640`; the TUI cover spec (out of scope, below) reuses it and the cache

### The cover cache

The player (never a client) downloads the current entry's cover and gives the desktop a local file, so every MPRIS consumer shows it, notification daemons that take only `file://` included (decision 6). This is the one on-disk cache the player keeps; 0009 decision 3 (no metadata cache) is otherwise unchanged.

- **Where**: the **cache directory**: `$TIDAL_PLAYER_CACHE_DIR`, else `$XDG_CACHE_HOME/tidal-player`, else `~/.cache/tidal-player`; covers in `covers/` (created `0700`), one file per cover ID: `<cover>.jpg`
- **When**: when an entry becomes current with a cover that is not cached, the player downloads it (one at a time; a newer current entry's cover replaces a waiting one; nothing is downloaded with no current entry or while MPRIS is off or unavailable). The download does not wait for the track: `Metadata` is published at once without `mpris:artUrl`, then again (`PropertiesChanged`) with the `file://` URL when the file is in place, if that entry is still current
- **Download**: `GET` the `640x640` URL with no Tidal credentials, a 10 s timeout and a 5 MB limit; anything but a `200` with an `image/*` body under the limit is a failure. The file is written as `<cover>.jpg.tmp`, then renamed, so the cache never holds half a file
- **Limit**: `max_cover_arts` in `app.toml` (default **20**, integer 0–1000), or `TIDAL_PLAYER_MAX_COVER_ARTS`, with 0008's precedence. After each new file, the least recently used covers are deleted until at most `max_cover_arts` remain; using a cached cover (it becomes current again) makes it the most recently used (its modification time is touched). The current entry's cover is never deleted. `0` turns the cache off: nothing is downloaded or written, and `mpris:artUrl` is the `https://` URL
- **Failures**: a failed download or an unwritable cache directory gives the `https://` URL for that entry (the desktop may still fetch it), is logged at `warn` once per cover, and is retried only when that cover becomes current again. Never the player's message: a missing cover must not cover a playback failure
- **Start**: files in `covers/` that are not `<uuid>.jpg` (a `.tmp` left by a crash, anything else) are deleted; the limit is applied once (a lowered `max_cover_arts` takes effect at the next start)
- `logout` leaves the cache: covers are public images, not the account's data

### Signals

- **`PropertiesChanged`** on `org.mpris.MediaPlayer2.Player`: after every player event that changed one or more of the properties above other than `Position`, one signal carrying exactly the changed properties with their new values (`Metadata` whole). Nothing when nothing changed: position events alone never emit it
- **`Seeked(position)`**: when the current entry's position jumps: it is the same entry as before and the new position is behind the last one, or ahead of it by more than the time elapsed since plus 1 s (a seek, `Previous`'s restart, repeat `track` starting over, a client's `SeekTo`). A change of current entry is a `Metadata` change, not a seek, unless the new entry starts at a position other than `0` (a restored position played, 0009)

### `org.mpris.MediaPlayer2.Player`: methods and writes

Each is mapped to one player `Command` and sent through the client hub, as a client's request (0005): the call returns once the player has handled it.

| Call | Command |
|---|---|
| `PlayPause()` | `TogglePause` |
| `Play()` | `Play` (new): plays when paused, stopped with a current entry, or held while loading; nothing when already playing or with no current entry |
| `Pause()` | `Pause` (new): pauses when playing, buffering or loading; nothing otherwise |
| `Stop()` | `Stop` (new): stops the engine (releasing the device), keeps the current entry, position `0:00`; `Play` then starts it from the beginning, as the MPRIS spec says |
| `Next()`, `Previous()` | `Next`, `Previous` |
| `Seek(offset µs)` | `SeekBy(offset in ms, rounded toward zero)`; nothing without a current entry |
| `SetPosition(trackid, position µs)` | `SetPosition { entry, position }` (new): applied only when `entry` is still the current entry and `0 ≤ position ≤ duration`, else nothing (the MPRIS spec: a stale `trackid` is ignored). A `trackid` that is not one of ours is ignored |
| `OpenUri(uri)` | `Open { items, at: None }` (0005): the URI parsed as an [item](../playback.md#items) (`https://tidal.com/browse/album/…`, `tidal://track/…`); replaces the queue and plays. An unparsable URI or an item kind `Open` refuses (an artist) is a D-Bus error with the parser's message |
| `Shuffle = b` | `SetShuffle(b)` (new): as `ToggleShuffle` when it differs, nothing when equal |
| `LoopStatus = s` | `SetRepeat(mode)` (new); an unknown string is an `InvalidArgs` error |
| `Volume = v` | `SetVolume(round(v × 100))`, clamped to 0–100 (so `v < 0` is `0`, as the MPRIS spec asks); `SetVolume` already unmutes (0004) |

The new commands are setters next to 0004's toggles. Two clients acting on stale views can still fight with them; MPRIS needs them because its clients state the result they want, and the player decides each against its current state, not the adapter's copy.

**One-shot commands** gain the same setters (0005 decision 6 deferred them here): `tidal-player playback play`, `pause`, `stop`, `shuffle on|off`, `repeat off|queue|track`. `shuffle` and `repeat` with no argument still toggle and cycle.

### Errors

| Situation | MPRIS answer |
|---|---|
| The player handled the command (whether or not it changed anything) | success |
| The player answered `Err(message)` (`Album 123 was not found` for `OpenUri`) | `org.mpris.MediaPlayer2.tidal_player.Error.Failed` with the message |
| No answer within 5 s (0005's one-shot deadline) | `org.freedesktop.DBus.Error.NoReply`-style failure: `org.mpris.MediaPlayer2.tidal_player.Error.Timeout` |
| A bad argument (`LoopStatus = "Shuffle"`, a non-object-path `trackid`) | `org.freedesktop.DBus.Error.InvalidArgs` |
| The player is shutting down | `org.mpris.MediaPlayer2.tidal_player.Error.Failed` `The player is shutting down` |

A failure that is the player's own (a track that cannot play, an output that is busy) is not an MPRIS error: it is the player's message, as for every client.

### Media keys

MPRIS is how media keys reach a player on Linux: GNOME and KDE route `XF86AudioPlay`/`Pause`/`Next`/`Prev`/`Stop` to the active MPRIS player, and other window managers bind them to `playerctl play-pause` and friends. `docs/mpris.md` shows the sway/i3 and Hyprland bindings. The TUI does not read media keys itself (decision 4).

## Acceptance criteria

Core (`tidal_player_core::player`, pure):

- **AC1** — The new commands (table over states: empty, stopped with a current entry, loading, loading held, playing, buffering, paused, paused and released): `Play` gives the same effects as `TogglePause` where `TogglePause` would start or resume, and none where it would pause; `Pause` the reverse; `Stop` sends `EngineStop` when the engine holds a track, keeps the current entry, sets position `0:00` and state `Stopped`, and is a no-op on a stopped player at `0:00`; a following `Play` starts it at `0:00`
- **AC2** — `SetShuffle`, `SetRepeat` and `SetPosition` (table): `SetShuffle(x)` equals `ToggleShuffle` when `x` differs and nothing otherwise (compared effect by effect and snapshot by snapshot); `SetRepeat(m)` sets the mode and reconciles the preload as `CycleRepeat` does; `SetPosition` on the current entry within its duration equals `SeekTo`, and on another entry, past the duration, or with no current entry does nothing. Every new `Command` variant joins `protocol::tests::all_commands` (JSON round-trip)

MPRIS model (`tidal-player::mpris::model`, pure, no D-Bus types: its own value enum):

- **AC3** — `player_properties(&snapshot, last_position)` (table over snapshots: empty; stopped with a current entry; loading; held load; playing with shuffle and repeat queue; buffering; paused; released; muted at 40 %; last entry with repeat off; last entry with repeat track; unknown duration): every property of the `Player` table, the `Can*` rules included
- **AC4** — `metadata(&snapshot)` (table): no current entry → `NoTrack` only; a track with a version, several artists, no album, unknown duration, and the same track queued twice (two trackids); `xesam:url`; `mpris:artUrl` for a track with a cover, none without
- **AC5** — `changes(old, new)` (table): no change → none; a position change alone → none; one property → that one; volume and mute together → `Volume` once; a track change → `Metadata` and the `Can*` that changed; `Shuffle` from `ToggleShuffle`, `SetShuffle` and a client's toggle alike
- **AC6** — `seeked(old, new, elapsed)` (table): forward by elapsed → none; backward → `Seeked`; forward by elapsed + 2 s → `Seeked`; another entry at `0` → none; another entry at a restored position → `Seeked`; no current entry → none
- **AC7** — `Position` (table, fake clock): last reported + elapsed while `Playing`, frozen while paused, buffering, loading or stopped; capped at the duration
- **AC8** — `command_for(call)` (table): every row of the methods-and-writes table, `Volume` `-0.5` → `0`, `1.7` → `100`, `0.333` → `33`; `Seek` of `-1500` µs → `SeekBy(-1)`; `SetPosition` with a foreign path → none; `LoopStatus` `"Shuffle"` → `InvalidArgs`; `OpenUri` with an artist link and with garbage → the parser's error

Adapter and runtime (`tidal-player`):

- **AC9** — The hub path (the in-process harness of 0005, fake engine, no bus: the adapter's hub side behind a seam): a call's command reaches the player as a request and the call completes after the player's `Reply`; a player `Err` becomes the `Failed` error with its message; a player that does not answer within the deadline (paused runtime, fake clock) gives `Timeout`; 100 calls in a burst are all applied, in order (none dropped: tidalt's non-blocking send)
- **AC10** — On the bus (a private `dbus-daemon --session` started by the test; the test fails, never skips, when `dbus-daemon` is missing): a daemon under the 0005 harness owns `org.mpris.MediaPlayer2.tidal_player`; `GetAll` on both interfaces matches AC3/AC4 for a loaded queue; `Pause()` on a paused player leaves it paused (tidalt's toggle); `Set(Volume, 0.5)` changes the volume and a subscribed client sees it; a client's `ToggleShuffle` emits one `PropertiesChanged` with `Shuffle` only; a `SeekTo` emits `Seeked`; `OpenUri` of a mock album loads it; `Quit()` leaves the daemon running; on `daemon stop` the name is released
- **AC11** — No bus (`DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus`, as 0005's tests already set): the daemon starts, serves clients and plays as before; the log has `MPRIS is not available:` once; the player's message is empty. 0005's existing daemon and CLI tests stay green unchanged
- **AC12** — The name taken (the test owns `org.mpris.MediaPlayer2.tidal_player` on the private bus first): the daemon takes `…tidal_player.instance<pid>` and works there
- **AC13** — `mpris` (0008's `app.toml` and precedence tables gain its rows: `true`, `false`, wrong type, `TIDAL_PLAYER_MPRIS=off` over a file's `true`, an invalid variable is an error): off → the private bus never sees a connection from the daemon (checked with `ListNames`/`NameOwnerChanged`)
- **AC14** — Clients never touch the bus: an attached TUI and `playback status` against a daemon with `mpris = false` leave the private bus with no new connection
- **AC15** — One-shot setters: `playback play|pause|stop`, `shuffle on|off`, `repeat off|queue|track` send `Play`, `Pause`, `Stop`, `SetShuffle`, `SetRepeat` (0005's argument table gains the rows; `shuffle maybe` exits 2); `shuffle` and `repeat` alone still send the toggles
- **AC16** — Shutdown order: on `Shutdown` the adapter stops taking calls (late calls get the shutting-down error) and the bus connection is closed after the clients got `ShuttingDown`

Cover art and its cache (`tidal-player-core`, `tidal-player-api`, `tidal-player`):

- **AC18** — The cover ID is parsed and kept (recorded fixtures, as 0002/0006): a track from `metadata/track.json`, an album page, a playlist page, favorite tracks and a search page has `album.cover == Some("…")`; a fixture row with `"cover": null` and one without the key → `None`. `cover_url` (table): a UUID → the URL with `/` for each `-` at `640x640`; other sizes as given. Every `Track` in `protocol::tests` gains a cover (JSON round-trip)
- **AC20** — The cover cache's decisions (`tidal-player::mpris::covers`, pure, table): which files to delete given the files with their last-use times, `max_cover_arts` and the current cover (21 files at 20 → the oldest; the current one oldest → the next oldest; 0 → none written and `https://`); the `artUrl` per state (cached → `file://`; downloading → none; failed or off → `https://`; no cover → none); the start clean-up (`.tmp` and stray names deleted). The cache directory's resolution (table: `TIDAL_PLAYER_CACHE_DIR`, `XDG_CACHE_HOME`, `HOME`)
- **AC21** — The cover cache's I/O (`tidal-player::mpris::covers`, temp dir, `wiremock`): a `200` `image/jpeg` → `covers/<cover>.jpg` via `.tmp` and a rename, mode `0600` in a `0700` directory; a `404`, a `text/html` body, a body over 5 MB and a response slower than the timeout (paused clock) → failure, no file left; a cached cover is not downloaded again and its modification time is touched; a second entry becoming current while the first cover downloads → only the newest is fetched next; the `Metadata` sequence on the adapter: without `artUrl`, then with the `file://` URL (one `PropertiesChanged`), and nothing when the entry changed meanwhile. `max_cover_arts` in 0008's `app.toml` and precedence tables (`20` default, `0`, `1000`, `1001` and `-1` errors, the variable over the file)
- **AC19** — Old state files: a `playback.json` written by 0009 (a fixture without `cover`) loads with `cover: None` everywhere and no `.bad` rename; a saved state with covers round-trips them (0009 AC1/AC6 tables gain the rows)

Docs:

- **AC17** — `docs/mpris.md` (what works, the bus name, the property and method tables in user terms, `playerctl` examples, media-key bindings for GNOME/KDE (nothing to do), sway/i3 and Hyprland, `mpris = false`, "no session bus" over SSH), `docs/daemon.md` (the new one-shot commands; `playerctl` works with the daemon), `docs/config.md` (`mpris`), `docs/playback.md` (link), their zh-TW copies, `README.md`/`README.zh-TW.md` and `CLAUDE.md` "Features"; the "MPRIS (0010)" notes in 0001, 0003, 0004, 0005, 0008 and 0009 point here. A test checks `mpris` and `max_cover_arts` appear in `docs/config.md` and the new one-shot commands in `docs/daemon.md` (0008 AC17's test, new rows)

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| No session bus (SSH, a systemd user service started before the session bus) | Runs without MPRIS, one `info` log line (AC11). Under systemd the user bus normally exists (`dbus.socket`); the unit gains no dependency |
| The session bus goes away while running (the session ends) | MPRIS stops working, one `warn` log line; the player keeps playing and serving socket clients; not retried |
| The name is taken | `.instance<pid>` (AC12) |
| A restored player (0009) | Published stopped, with the restored current entry's `Metadata` and `CanPlay` true: a desktop widget can start it |
| Empty queue | `Stopped`, `NoTrack`, every `Can*` but `CanControl` false; calls are accepted and do nothing |
| Login required (0002) | MPRIS unchanged; `Play` fails as any play does, with the player's message |
| The cache directory is read-only, full or missing permissions | `https://` URLs, one `warn` per cover; playback unaffected |
| Two players with different runtime dirs share one cache directory | Both may write the same `<cover>.jpg`; the rename makes either write whole; eviction by one may delete the other's file, which is downloaded again when needed |
| A cover changes on Tidal under the same ID | Never: Tidal's cover IDs name an image; the cached file is used as is |
| A track with unknown duration | No `mpris:length`, `CanSeek` false, `SetPosition` ignored, `Seek` still sent (0004 handles it) |
| Device released while paused (0005) | `Paused`; `Play` reopens the device as `space` does; a failed reacquire stays `Paused` with the player's message |
| A widget polls `Position` many times a second | Answered from the adapter's copy, never by asking the player |
| A client mashes `Volume` writes | Each is a request; the player coalesces nothing, the persistence (0009) does |
| `Seek` past the end | `SeekBy` (0004: seeking past the end moves to the next track, as the MPRIS spec asks) |
| Two players on one bus (different runtime dirs) | Each has its own name; `playerctl` picks one by its own rules |
| `tidal-player play` over SSH with X forwarding or a forwarded bus | It publishes on whatever bus `DBUS_SESSION_BUS_ADDRESS` names; documented |

## Test plan

Each test is named after its criterion (`ac3_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0009.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/player/tests/setters.rs` :: `ac1_play_pause_stop` (table) | effects and snapshot per state | stub `Play`/`Pause`/`Stop` do nothing |
| AC2 | `crates/core/src/player/tests/setters.rs` :: `ac2_set_shuffle_repeat_position` (table); `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) | equal to the toggles / `SeekTo`; round-trip | stub setters do nothing |
| AC3 | `crates/app/src/mpris/model.rs` :: `ac3_player_properties` (table) | every property per snapshot | stub returns tidalt's constants (`Can*` true, `CanSeek` false) |
| AC4 | `crates/app/src/mpris/model.rs` :: `ac4_metadata` (table) | keys and values | stub returns `NoTrack` only |
| AC5 | `crates/app/src/mpris/model.rs` :: `ac5_changes` (table) | the changed set | stub returns no changes (tidalt) |
| AC6 | `crates/app/src/mpris/model.rs` :: `ac6_seeked` (table) | when `Seeked` fires | stub never fires |
| AC7 | `crates/app/src/mpris/model.rs` :: `ac7_position` (table, fake clock) | extrapolated position | stub returns the last report |
| AC8 | `crates/app/src/mpris/model.rs` :: `ac8_command_for` (table) | command or error per call | stub maps `Play` and `Pause` to `TogglePause` (tidalt) |
| AC9 | `crates/app/src/mpris/hub.rs` :: `ac9_calls_reach_player`, `ac9_error_and_timeout`, `ac9_burst_in_order` | replies, errors, nothing dropped | stub completes calls before the reply and drops on a full channel |
| AC10 | `crates/app/tests/mpris.rs` :: `ac10_on_the_bus` | the bus scenario | stub adapter never connects |
| AC11 | `crates/app/tests/mpris.rs` :: `ac11_no_bus`; the 0005 tests unchanged | runs, logs once, no message | stub exits when the bus is missing |
| AC12 | `crates/app/tests/mpris.rs` :: `ac12_name_taken` | `.instance<pid>` | stub gives up when the name is taken |
| AC13 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows); `crates/app/tests/mpris.rs` :: `ac13_mpris_off` | parsing, precedence, no connection | stub ignores the setting |
| AC14 | `crates/app/tests/mpris.rs` :: `ac14_clients_stay_off_the_bus` | no new bus connection from clients | stub client publishes too |
| AC15 | `crates/app/src/oneshot.rs` :: the 0005 argument table (new rows) | parsed commands, exit 2 | stub parses only the toggles |
| AC16 | `crates/app/src/mpris/hub.rs` :: `ac16_shutdown_order` | refusal after `Shutdown`; close after `ShuttingDown` | stub closes first |
| AC18 | `crates/api/src/metadata.rs`, `crates/api/src/library.rs` :: `ac18_cover` (table, fixtures); `crates/core/src/track.rs` :: `ac18_cover_url` (table) | parsed covers; URLs | stub DTOs drop the field; stub `cover_url` returns `""` |
| AC19 | `crates/app/src/persist.rs` :: `ac6_load` (new row); `crates/core/src/player/tests/persistence.rs` :: `ac1_saved_round_trip` (new row) | old file loads; covers survive | stub `cover` without `#[serde(default)]` fails the old file |
| AC20 | `crates/app/src/mpris/covers.rs` :: `ac20_eviction`, `ac20_art_url`, `ac20_start_cleanup`, `ac20_cache_dir` (tables) | what is deleted; the URL per state; the directory | stub keeps every file and always gives `https://` |
| AC21 | `crates/app/src/mpris/covers.rs` :: `ac21_download`, `ac21_download_failures`, `ac21_latest_wins`, `ac21_metadata_sequence`; `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows) | files, modes, failures, order, signals, settings | stub download writes in place and never fails |
| AC17 | `crates/app/tests/docs.rs` :: `ac17_every_command_documented` (new rows) + reviewed at acceptance | docs | stub docs lack the rows |

Checked by hand at acceptance on the user's machine (results in the PR description), because they need a real desktop: GNOME's (or KDE's) media widget shows the track, updates on track change, pause and seek without delay, and its buttons work, and it shows the album cover; the keyboard's play/pause, next and previous keys control the daemon with no TUI open; `playerctl -p tidal_player metadata --follow` follows track changes; a Bluetooth headset's buttons control playback; the device is released after pausing from the widget (0005).

CI: `dbus-daemon` comes with the `dbus` package; the CI job's `apt-get install` gains `dbus` so AC10–AC14 run there (they fail, never skip, without it).

## Crate placement

- `tidal-player-core`: `AlbumRef::cover` (`#[serde(default)]`) and `track::cover_url`; the new `Command` variants (`Play`, `Pause`, `Stop`, `SetShuffle(bool)`, `SetRepeat(RepeatMode)`, `SetPosition { entry, position }`) and their handling in `player`. No D-Bus, no new dependency
- `tidal-player-api`: the `cover` field in the metadata and library album DTOs, mapped into `AlbumRef`
- `tidal-player` (binary): `mpris/model.rs` (pure: properties, metadata, changes, `Seeked`, position, call → command, with its own value type), `mpris/hub.rs` (the adapter as an in-process client of the hub: a `Peer` with no stream that subscribes and sends `Request`s with ids, waiting for each `Reply` with a deadline), `mpris/bus.rs` (the thin zbus layer: name, object, interfaces, signals; checked by AC10–AC14 and by hand), `mpris/covers.rs` (the cache: pure eviction and URL rules, the download over `reqwest` behind a seam), wired into the standalone, `daemon` and `play` players; `mpris`, `max_cover_arts` and the cache directory in `config`; the one-shot setters in `oneshot`. `zbus` is already a dependency (blocking API for 0003's reservation); the adapter may use its async API on the player's tokio runtime (a workspace feature change only)
- `xtask layering`: no change (core and audio still have no D-Bus)
- Adding a field to `AlbumRef` touches every test that builds one with a struct literal; the slice that adds it updates them (or they use a helper)

## Facts vs. assumptions

Verified (2026-10-09, from code):

- tidalt: as under "What tidalt did" (files and commits named there)
- This repo: the hub accepts in-process peers (`Peer::new(outbox, None)`) and orders their `Subscribe`/`Request` with every other input (`crates/app/src/ipc/server.rs`); `TogglePause` on a stopped player starts the current entry at `position`; `Next` with repeat on wraps (`skip_target`), and with nothing after it stops; `SetVolume` unmutes; `zbus` 5 is a workspace dependency with `blocking-api` and `async-io`; 0005's daemon and CLI tests set `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus`; `dbus-daemon` is installed in this development container; the recorded v1 track objects carry `album.cover` as a UUID next to `vibrantColor` and `videoCover` (`crates/api/tests/fixtures/metadata/track.json`, `album_page.json`, `playlist_page_with_video.json`, and three library fixtures), and our DTOs drop it (`metadata.rs` and `library.rs` `AlbumDto`); the player has no cache directory yet (only the state directory, `store_setup.rs`)

Assumptions, checked at acceptance:

- The MPRIS2 specification (2.2): bus name `org.mpris.MediaPlayer2.<name>`, `.instance<pid>` for further instances; `/org/mpris` paths are reserved except `/org/mpris/MediaPlayer2/TrackList/NoTrack`; `Position` is not signalled by `PropertiesChanged` and `Seeked` covers jumps; `SetPosition` ignores a stale `trackid`; `Volume` below 0 is treated as 0; `Stop` then `Play` starts the track from the beginning. Re-read against the spec text before AC3–AC8 are written
- The image URL `https://resources.tidal.com/images/<a/b/c/d/e>/640x640.jpg` (tidalt used it; sizes `80x80`, `160x160`, `320x320`, `640x640`, `1280x1280`) needs no authentication. `resources.tidal.com` is not reachable from the development container, so it is checked on the user's machine before AC18 is written (`curl -I` on a cover from a real track)
- GNOME Shell's and KDE Plasma's media widgets load an `https://` `mpris:artUrl` themselves (some notification daemons only take `file://`; they show no cover)
- `playerctl`, `gsd-media-keys` and KDE's media controller pick up a player from `PropertiesChanged` alone, with no `DesktopEntry`
- `ubuntu-latest` runners can install `dbus` and run a private `dbus-daemon --session` without a system bus
- spotify-player (from memory, re-check): MPRIS through the `souvlaki` crate behind its `media-control` feature, on by default on Linux, bus name `org.mpris.MediaPlayer2.spotify_player`

## Decisions (answered by the user, 2026-10-09: 2 and 6 as noted, the rest as proposed)

1. **Bus name** `org.mpris.MediaPlayer2.tidal_player` (spotify-player's style: `playerctl -p tidal_player`). Alternative: `tidal-player` (hyphens are legal but discouraged in bus names)
2. **Every player publishes**, `play` included, on by default (`mpris = true`), `mpris = false` to turn it off. *Answered (the user, 2026-10-09): on by default, `mpris = true`*
3. **Setters in core** (`Play`, `Pause`, `Stop`, `SetShuffle`, `SetRepeat`, `SetPosition`), decided against the player's real state, and the same setters as one-shot commands. Alternative: the adapter computes toggles from its copy (racy; tidalt's bug class)
4. **The TUI does not read media keys**: the desktop routes them through MPRIS, and a terminal only sees them with the kitty keyboard protocol in a few terminals. Alternative: map crossterm's media key codes in `keymap.toml`
5. **`Quit` does nothing** (`CanQuit = false`), so a widget cannot stop a daemon. Alternative: `CanQuit = true` for the standalone TUI only
6. **Cover art included, cached, no `TrackList`**: *answered (the user, 2026-10-09): cover art included; "cover caches are limited by `max_cover_arts` default to 20"; the TUI cover is a separate spec.* The cover ID is added to `Track`'s album; the player downloads the current cover into the cache directory (at most `max_cover_arts`, default 20, least recently used evicted) and gives a `file://` URL, Tidal's `https://` URL when it cannot. `HasTrackList = false`
7. **`Loading` reads as `Playing`** (the user asked to play; widgets show a pause button). Alternative: `Paused` until the stream starts
8. **Muted reads as `Volume = 0.0`**, and a written `Volume` unmutes (as `SetVolume` does). Alternative: report the volume and ignore mute

## Out of scope

- `org.mpris.MediaPlayer2.TrackList` and `Playlists` (decision 6)
- **The cover in the TUI** (spotify-player's `image` feature, tidalt's Kitty/sixel panel): its own spec, reusing this spec's `cover`, `cover_url` and cover cache
- Prefetching the next entry's cover; other image sizes; a `.desktop` file (`DesktopEntry`)
- Media keys read by the TUI itself (decision 4)
- Autoplay over MPRIS (no standard property)
- MPRIS on macOS or Windows (the player is Linux-only, 0003)
- Mixes and radio (0011), filter and sort (0012)
