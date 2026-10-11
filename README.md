English | [正體中文](https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/master/README.zh-TW.md)

# tidal-player

[![crates.io](https://img.shields.io/crates/v/tidal-player.svg)](https://crates.io/crates/tidal-player)
[![CI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/workflows/ci.yml/badge.svg)](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A terminal player for [Tidal](https://tidal.com) on Linux, in the spirit of [spotify-player](https://github.com/aome510/spotify-player): your library, search and mixes in a keyboard-driven TUI, with bit-perfect playback straight to your DAC.

![Playing an album in tidal-player](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-playing-song.png)

## Features

- **Bit-perfect playback**: streams the best quality Tidal offers (up to `HI_RES_LOSSLESS` FLAC) and plays it on an ALSA `hw:` device at the track's own rate, taking the card from PipeWire while it plays. Gapless, with the next track preloaded. [Playback](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Playback)
- **Your library**: favorite tracks, playlists, albums and artists, with album, playlist and artist pages; edit favorites and your playlists from the actions popup. [The TUI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/TUI)
- **Search, mixes and radio**: search tracks, albums, artists and playlists (`g s`), open your daily mixes (`g m`) or the radio of a track or artist (`r`), and filter any list as you type (`/`)
- **A real queue**: shuffle, repeat, autoplay with Tidal's suggestions when the queue runs out, and the last session (queue, position, modes, volume) restored when you start again
- **Daemon and clients**: run the player headless as a systemd user service, attach any number of TUIs, and control it with one-shot `tidal-player playback …` commands. The audio device is released while paused. [Daemon](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Daemon)
- **Desktop controls**: media keys, Bluetooth headsets, GNOME/KDE media controls and `playerctl` over MPRIS, album cover included. [MPRIS](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/MPRIS)
- **Output devices**: switch the output device while a track plays (`D` in the TUI, `tidal-player playback device` from a shell), without losing your place
- **Your keys**: a spotify-player-compatible `keymap.toml`, a keys help (`?`) that lists the keys where you are and runs them, and hints that show what can follow `g` while you wait. [Configuration](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Configuration)

## Screenshots

*(The font is Ink Free. If I got a penny every time someone complained about it, I'd be a billionaire by now.)*

Press `g` and wait: a hint lists the keys that can follow.

![Key hints after g](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-library-with-goto-hints.png)

`?` opens the keys help: the keys for where you are first, filterable, and `enter` runs one.

![The keys help](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-with-help.png)

`D` lists the output devices; `enter` moves playback there at the same position.

![Choosing the output device](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-device-choosing.png)

## Requirements

- Linux with ALSA. MPRIS needs a D-Bus session bus, and the session is kept in the system keyring when there is one (otherwise in an encrypted file)
- A paid Tidal subscription; hi-res streams need a plan that includes them
- To build: Rust 1.97 or newer, `pkg-config` and the ALSA headers

| Distribution | Build dependencies |
|---|---|
| Debian, Ubuntu | `sudo apt install pkg-config libasound2-dev` |
| Arch | `sudo pacman -S pkgconf alsa-lib` |
| Fedora | `sudo dnf install pkgconf-pkg-config alsa-lib-devel` |

## Installation

```sh
cargo install tidal-player
```

Or download the x86_64 Linux binary from [Releases](https://github.com/PEKKA1117/spotify-player-but-for-tidal/releases): each release lists its install steps, with SHA-256 checksums.

Or from a checkout of this repository:

```sh
cargo install --path crates/app
```

## Quick start

```sh
tidal-player login   # open the link, enter the code
tidal-player         # the TUI
```

In the TUI, `g l` opens your library, `g s` search and `g m` your mixes; `enter` plays, `space` pauses, `n`/`p` skip, `?` lists every key and `q` quits. More in [The TUI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/TUI).

For a bit-perfect output, find your DAC with `tidal-player devices` and pick it with `D` in the TUI, or set `output_device` in `app.toml` (see [Finding your DAC's device](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Playback#finding-your-dacs-device)).

To keep the player running in the background, install it as a systemd user service:

```sh
mkdir -p ~/.config/systemd/user
tidal-player daemon unit > ~/.config/systemd/user/tidal-player.service
systemctl --user daemon-reload
systemctl --user enable --now tidal-player
```

Then `tidal-player` attaches a TUI to it, and `tidal-player playback next` (and the other `playback` commands) control it from a shell or a keybinding.

## Configuration

Settings live in `~/.config/tidal-player/app.toml` and keys in `~/.config/tidal-player/keymap.toml`. The [example `app.toml`](https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/master/examples/app.toml) and [example `keymap.toml`](https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/master/examples/keymap.toml) spell out every default: copy them and edit. Environment variables and command-line flags still work and take precedence. Every setting and command is in [Configuration](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Configuration).

## Documentation

The user guide is on the [wiki](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki):
[The TUI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/TUI) ·
[Configuration](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Configuration) ·
[Playing tracks](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Playback) ·
[Logging in](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Login) ·
[The daemon](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Daemon) ·
[Desktop controls and media keys](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/MPRIS)

The design of each feature is specified in [`docs/specs`](https://github.com/PEKKA1117/spotify-player-but-for-tidal/tree/master/docs/specs).

## Acknowledgements

- [spotify-player](https://github.com/aome510/spotify-player) by aome510, whose interface, keys and `keymap.toml` format this player follows
- [tidalt](https://github.com/Benehiko/tidalt) by Benehiko, the earlier Tidal TUI this project was rebuilt from: what it learned about Tidal's API and bit-perfect ALSA output shaped this one

## Disclaimer

tidal-player is an unofficial client. It is not affiliated with, endorsed or sponsored by TIDAL. You need your own Tidal subscription, and you use it under Tidal's terms.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/master/LICENSE-APACHE))
- MIT license ([LICENSE-MIT](https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/master/LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
