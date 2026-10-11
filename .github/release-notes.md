## Install

Needs Linux with ALSA and a paid Tidal subscription (see [Requirements](https://github.com/${GITHUB_REPOSITORY}/blob/${TAG}/README.md#requirements)).

### Prebuilt binary (x86_64 Linux)

```sh
base=https://github.com/${GITHUB_REPOSITORY}/releases/download/${TAG}
curl -LO "$base/tidal-player-${TAG}-x86_64-unknown-linux-gnu.zip" -LO "$base/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS
unzip tidal-player-${TAG}-x86_64-unknown-linux-gnu.zip
install -Dm755 tidal-player-${TAG}-x86_64-unknown-linux-gnu/tidal-player ~/.local/bin/tidal-player
```

The binary links the system ALSA library (`libasound.so.2`, package `libasound2` on Debian and Ubuntu, `alsa-lib` on Arch and Fedora).

### From crates.io

Needs Rust 1.97 or newer, `pkg-config` and the ALSA headers ([build dependencies](https://github.com/${GITHUB_REPOSITORY}/blob/${TAG}/README.md#requirements)):

```sh
cargo install tidal-player --version ${VERSION}
```

### From source

Unpack `tidal-player-${TAG}-src.zip`, then in its directory:

```sh
cargo install --path crates/app
```

Then sign in and play: see [Quick start](https://github.com/${GITHUB_REPOSITORY}/blob/${TAG}/README.md#quick-start).
