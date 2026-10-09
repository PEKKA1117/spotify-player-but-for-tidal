English | [繁體中文](zh-TW/mpris.md)

# Desktop controls and media keys (MPRIS)

The player publishes itself on your session bus as an **MPRIS2** media player, the D-Bus interface every Linux desktop uses for its media controls. With it, these control tidal-player and show what it plays, the album cover included:

- GNOME's and KDE Plasma's media controls (the top bar, the lock screen, notifications)
- bars such as waybar and polybar, and KDE Connect
- the keyboard's media keys (play/pause, next, previous, stop)
- Bluetooth headsets' buttons (through BlueZ)
- `playerctl`

Every player publishes itself: the standalone TUI, `tidal-player daemon` and `tidal-player play`. An attached TUI and `tidal-player playback …` are clients of that player and never touch the bus. Design: [spec 0010](specs/0010-mpris.md).

## The bus name

The player owns `org.mpris.MediaPlayer2.tidal_player` (`playerctl -p tidal_player`). A second player on the same bus (one started with another `TIDAL_PLAYER_RUNTIME_DIR`) takes `org.mpris.MediaPlayer2.tidal_player.instance<pid>` instead, as the MPRIS specification asks.

## `playerctl`

```sh
playerctl -p tidal_player play-pause
playerctl -p tidal_player next
playerctl -p tidal_player pause          # pauses; a paused player stays paused
playerctl -p tidal_player position 90    # 1:30 into the track
playerctl -p tidal_player position 10+   # 10 s forward
playerctl -p tidal_player volume 0.5     # 50 %
playerctl -p tidal_player shuffle On
playerctl -p tidal_player loop Playlist  # repeat the queue (None, Playlist, Track)
playerctl -p tidal_player open https://tidal.com/browse/album/123
playerctl -p tidal_player metadata --follow
```

`open` takes what [`tidal-player playback load`](playback.md#items) takes (a track, album or playlist link, `tidal://…`): it replaces the queue and plays.

## What the desktop sees

| Shown as | From the player |
|---|---|
| Playing, Paused, Stopped | playing (also while a track loads or buffers), paused (also while the device is released), stopped |
| Title | the track's title, with its version in parentheses (`Hell Above (Live)`) |
| Artist | every artist of the track |
| Album, cover | the album and its cover (see [Album covers](#album-covers)) |
| Length, position | the track's duration and position; a seek is announced at once |
| Volume | the volume (`0` while muted); setting it unmutes |
| Shuffle, loop | shuffle; repeat `off`, `queue`, `track` as `None`, `Playlist`, `Track` |
| Next / previous buttons | next is enabled when there is a track after the current one, or repeat is on; previous whenever a track is current |

The buttons and commands act on the player's real state, as `Space`, `n` and `p` do in the TUI:

| Command | Does |
|---|---|
| Play | plays when paused or stopped; nothing when already playing |
| Pause | pauses when playing; nothing when already paused |
| Play/pause | toggles, as `Space` |
| Stop | stops, releases the device and goes back to `0:00`; Play then starts the track from the beginning |
| Next, previous | as `n`, `p` |
| Seek, set position | as the seek keys; a position past the track's end moves to the next track |
| Quit, raise | nothing: a desktop widget cannot stop the daemon (`tidal-player daemon stop` and `q` do) |

A command the player cannot carry out answers with the player's error (`Album 123 was not found`); a player that does not answer within 5 s answers with a timeout. A failure of the track itself (an output that is busy, a track that is not available) is the player's message, as in the TUI.

A restored player ([Resuming the last session](playback.md#resuming-the-last-session)) is published stopped, with the restored track, so the desktop's play button starts it.

## Album covers

The player downloads the current track's album cover into its **cache directory** and gives the desktop the file, so every widget and notification shows it:

- **Where**: `$TIDAL_PLAYER_CACHE_DIR`, else `$XDG_CACHE_HOME/tidal-player`, else `~/.cache/tidal-player`; the covers are in `covers/`, one `<cover>.jpg` each (640×640)
- **How many**: at most `max_cover_arts` (in [`app.toml`](config.md#apptoml), default `20`, `0` to `1000`), or `TIDAL_PLAYER_MAX_COVER_ARTS`; the least recently used go first. A lowered limit takes effect at the next start
- **`max_cover_arts = 0`**: nothing is downloaded or written; the desktop gets Tidal's image address and fetches it itself (GNOME and KDE do; some notification daemons only show local files)
- While a cover downloads, the track shows without one, then with it. A cover that cannot be downloaded or written is shown from Tidal's address instead, and the log says why once
- `tidal-player logout` leaves the cache: covers are public images, not your account's data. Delete the directory to empty it

## Media keys

**GNOME, KDE Plasma** and other full desktops: nothing to do. The media keys go to the player that last played.

**sway, i3** (`~/.config/sway/config` or `~/.config/i3/config`):

```
bindsym XF86AudioPlay exec playerctl -p tidal_player play-pause
bindsym XF86AudioPause exec playerctl -p tidal_player pause
bindsym XF86AudioStop exec playerctl -p tidal_player stop
bindsym XF86AudioNext exec playerctl -p tidal_player next
bindsym XF86AudioPrev exec playerctl -p tidal_player previous
```

**Hyprland** (`~/.config/hypr/hyprland.conf`; `bindl` works on the lock screen too):

```
bindl = , XF86AudioPlay, exec, playerctl -p tidal_player play-pause
bindl = , XF86AudioPause, exec, playerctl -p tidal_player pause
bindl = , XF86AudioStop, exec, playerctl -p tidal_player stop
bindl = , XF86AudioNext, exec, playerctl -p tidal_player next
bindl = , XF86AudioPrev, exec, playerctl -p tidal_player previous
```

Leave out `-p tidal_player` to control whichever player `playerctl` picks. The TUI itself does not read media keys: in a terminal they rarely arrive, and the desktop routes them here anyway.

## Turning it off

`mpris = false` in [`app.toml`](config.md#apptoml), or `TIDAL_PLAYER_MPRIS=off`: the player does not connect to the session bus for MPRIS (and downloads no covers). Playback, the TUI and the daemon's clients are unaffected.

## No session bus

Over SSH, on a headless box or in a container there is often no session bus. The player then runs as before without MPRIS; the daemon's log (`journalctl --user -u tidal-player`) says `MPRIS is not available: …` once. It is not an error, and it is not retried while the player runs: restart the player once a bus exists.

Under systemd the daemon reaches your user session's bus (`dbus.socket`). `tidal-player play` over SSH publishes on whatever bus `DBUS_SESSION_BUS_ADDRESS` names (a forwarded one, for instance).

## Troubleshooting

| Symptom | What to check |
|---|---|
| `playerctl -l` does not list `tidal_player` | Is a player running (`tidal-player playback status`)? Is `mpris` off? Does the player see the same bus as your desktop (the daemon's log says `MPRIS is not available` otherwise)? |
| The media keys control another player | On GNOME/KDE, play something in tidal-player once. With `playerctl` bindings, add `-p tidal_player` |
| No cover | The track's album has none on Tidal, or the cover could not be downloaded (the log says why); `max_cover_arts = 0` leaves fetching to the desktop |
