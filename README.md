English | [繁體中文](README.zh-TW.md)

vibe WIP tui project for tidal mimicing [spotify-player](https://github.com/aome510/spotify-player)

What works today: logging in with Tidal's device flow, bit-perfect playback to ALSA `hw:` devices, a queue with shuffle, repeat and autoplay, a terminal UI with your library (favorites, playlists, albums, artists) and search, a headless daemon (systemd user service) that TUIs attach to and one-shot `tidal-player playback …` commands control, resuming the last session (queue, position, modes and volume) stopped where it was after a restart, and configuration files: `~/.config/tidal-player/app.toml` for the settings (environment variables and flags still work and win) and a spotify-player-compatible `keymap.toml` for the keys, with a keys help popup (`?`) that lists the keys where you are, filters them and runs them.

Default config files to copy and edit: [`examples/app.toml`](examples/app.toml), [`examples/keymap.toml`](examples/keymap.toml) (they spell out every default, so as shipped they change nothing).

Docs: [the TUI](docs/tui.md), [configuration](docs/config.md) (`app.toml`, `keymap.toml`, every command), [playing tracks](docs/playback.md), [logging in](docs/login.md), [the daemon](docs/daemon.md).

Design specs: [docs/specs](docs/specs).
