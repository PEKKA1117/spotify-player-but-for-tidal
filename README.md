English | [繁體中文](README.zh-TW.md)

vibe WIP tui project for tidal mimicing [spotify-player](https://github.com/aome510/spotify-player)

What works today: logging in with Tidal's device flow, bit-perfect playback to ALSA `hw:` devices, a queue with shuffle, repeat and autoplay, a terminal UI with your library (favorites, playlists, albums, artists), search, a [filter](docs/tui.md#filtering-a-list) (`/`) that narrows any list or the queue as you type, your Tidal mixes and track or artist radio (`g m`, `r`), a headless daemon (systemd user service) that TUIs attach to and one-shot `tidal-player playback …` commands control, resuming the last session (queue, position, modes and volume) stopped where it was after a restart, desktop media controls, media keys and `playerctl` over MPRIS (with the album cover), and configuration files: `~/.config/tidal-player/app.toml` for the settings (environment variables and flags still work and win) and a spotify-player-compatible `keymap.toml` for the keys, with a keys help popup (`?`) that lists the keys where you are, filters them and runs them, and key hints that show what can follow the first key of a sequence (`g …`) while you wait.

Default config files to copy and edit: [`examples/app.toml`](examples/app.toml), [`examples/keymap.toml`](examples/keymap.toml) (they spell out every default, so as shipped they change nothing).

Docs: [the TUI](docs/tui.md), [configuration](docs/config.md) (`app.toml`, `keymap.toml`, every command), [playing tracks](docs/playback.md), [logging in](docs/login.md), [the daemon](docs/daemon.md), [desktop controls and media keys](docs/mpris.md).

Design specs: [docs/specs](docs/specs).
