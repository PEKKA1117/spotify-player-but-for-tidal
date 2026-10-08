vibe WIP tui project for tidal mimicing [spotify-player](https://github.com/aome510/spotify-player)

What works today: logging in with Tidal's device flow, bit-perfect playback to ALSA `hw:` devices, a queue with shuffle, repeat and autoplay, a terminal UI with your library (favorites, playlists, albums, artists) and search, and a headless daemon (systemd user service) that TUIs attach to and one-shot `tidal-player playback …` commands control.

Docs: [the TUI](docs/tui.md), [playing tracks](docs/playback.md), [logging in](docs/login.md), [the daemon](docs/daemon.md).

Next up: config files (`app.toml`, spotify-player-compatible `keymap.toml`) and a keys help popup ([spec 0008](docs/specs/0008-keymap-and-config.md), approved). Until then settings come from environment variables ([Settings](docs/playback.md#settings)) and keys are fixed ([Keys](docs/tui.md#keys)).

Design specs: [docs/specs](docs/specs).
