# 0001 — Architecture & project scaffold

- **Status**: draft
- **Owner**: tech-lead (primary session)

## Context

tidalt (Go + CGO + FFmpeg + ALSA) grew too buggy to keep patching. Its problems were structural as much as local:

- UI state, I/O and playback were tangled in one large BubbleTea model, so most behaviour could only be tested by driving the whole UI
- CGO + a dynamically or statically linked FFmpeg made builds, CI and packaging fragile
- `CLAUDE.md` became a per-file changelog instead of a spec, so intended behaviour lived nowhere testable

This spec picks the stack and the crate layout, and sets up an empty workspace in which every later spec can be built test-first. It adds **no user-facing features** beyond `--help`/`--version` and an empty TUI frame.

## Decisions

### Language & toolchain

- Rust, edition 2024, stable toolchain pinned in `rust-toolchain.toml` (currently 1.97), with `rustfmt` and `clippy` components
- **Linux only** (ALSA output), as tidalt was. Nothing outside `crates/audio`'s ALSA backend may be Linux-specific

### Workspace layout

```
Cargo.toml                 # [workspace], shared [workspace.dependencies] and [workspace.lints]
rust-toolchain.toml
crates/
  core/    -> tidalt-core  # domain types, app state, Action, Effect, update(), keymap. NO I/O.
  tidal/   -> tidalt-tidal # Tidal HTTP API client; maps API DTOs into core types
  audio/   -> tidalt-audio # decode + output engine; Sink trait, ALSA backend behind a feature
  tui/     -> tidalt       # the binary: CLI, terminal, event loop, rendering, effect runner
scripts/check-layering.sh  # enforces the dependency rules below via `cargo tree`
.github/workflows/ci.yml
```

Allowed dependency edges (everything else is forbidden):

| crate          | may depend on                         |
|----------------|---------------------------------------|
| `tidalt-core`  | — (only pure libs: serde, thiserror…) |
| `tidalt-tidal` | `tidalt-core`                         |
| `tidalt-audio` | — (not `core`, not `tidal`)           |
| `tidalt`       | all of the above                      |

### Application pattern (the main testability fix)

Elm-style, as a pure core with an imperative shell:

- `tidalt-core` defines `State`, `Action` (everything that can happen: key presses already resolved through the keymap, API responses, player events, ticks) and `Effect` (everything the app wants done: fetch X, play Y, persist Z, quit)
- `fn update(state: &mut State, action: Action) -> Vec<Effect>` is pure and synchronous. All behaviour that can be decided without I/O is decided here and unit-tested here
- The binary's runtime turns terminal events, API results and player events into `Action`s, calls `update`, and executes the returned `Effect`s. It contains no decisions worth testing beyond wiring
- Rendering is `fn render(&State, &mut Frame)`: a pure function of state, tested with ratatui's `TestBackend` + `insta` snapshots at fixed sizes

### Concurrency

- `tokio` multi-thread runtime in the binary for HTTP and the event loop
- Audio decode + output run on a **dedicated OS thread** owned by `tidalt-audio` (never on the async runtime), controlled through a command channel and reporting through an event channel. `tidalt-audio` exposes a synchronous API and does not depend on tokio

### Key libraries

| purpose            | crate                                          |
|--------------------|------------------------------------------------|
| TUI                | `ratatui` 0.30 + `crossterm` 0.29              |
| async / HTTP       | `tokio` 1, `reqwest` 0.13 (rustls)             |
| decoding           | `symphonia` 0.6 (FLAC, ALAC, AAC, MP4) — no FFmpeg |
| audio output       | `alsa` 0.12 (direct `hw:` for bit-perfect)     |
| CLI                | `clap` 4 (derive)                              |
| config / serde     | `serde`, `toml`, `directories`                 |
| secrets            | `keyring` 4                                    |
| MPRIS (later spec) | `zbus` 5 / `mpris-server`                      |
| errors             | `thiserror` in libraries, `anyhow` in the binary |
| logging            | `tracing` + `tracing-appender` to a file under the XDG state dir; never stdout/stderr while the TUI is up |
| tests              | `insta` (snapshots), `wiremock` (HTTP fixtures), `assert_cmd` (CLI) |

### Build & tooling (copied into `CLAUDE.md` when this spec is approved)

- **Format**: `cargo fmt --all` (CI: `cargo fmt --all --check`)
- **Lint**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`, plus `scripts/check-layering.sh`
- **Build**: `cargo build --workspace`
- **Test**: `cargo test --workspace`; one crate: `cargo test -p tidalt-core`; one test: `cargo test -p tidalt-core <name>`. Snapshot changes are reviewed, never blindly accepted (`cargo insta review`, or inspect the `.snap.new` files)
- **System deps**: `pkg-config`, `libasound2-dev` (only for the `alsa` feature). `cargo test -p tidalt-audio --no-default-features` builds without them

## Acceptance criteria

- **AC1** — On a clean checkout with the system deps installed, `cargo build --workspace` succeeds and produces a `tidalt` binary
- **AC2** — `cargo fmt --all --check`, the clippy command above, and `cargo test --workspace` all pass
- **AC3** — `scripts/check-layering.sh` exits 0 on the scaffold and non-zero (naming the offending edge) when any forbidden edge in the table is added, and when `tidalt-core` gains any of: `tokio`, `reqwest`, `crossterm`, `ratatui`, `alsa`, `symphonia`, `zbus`, `keyring`
- **AC4** — `tidalt --version` prints `tidalt <version>` and exits 0; `tidalt --help` exits 0 (tested with `assert_cmd`)
- **AC5** — `tidalt-core` exposes `State`, `Action`, `Effect` and `update`; `update(&mut State::default(), Action::Quit)` returns `[Effect::Quit]`, and any other action on the default state returns no effects (unit test)
- **AC6** — `render` of `State::default()` into an 80×24 `TestBackend` matches a committed `insta` snapshot showing an empty layout with the app name
- **AC7** — `tidalt-audio` defines a `Sink` trait with an in-memory test sink; the ALSA backend is behind a default-on `alsa` feature, and `cargo test -p tidalt-audio --no-default-features` passes without `libasound2-dev`
- **AC8** — `tidalt-tidal` has an injectable base URL and one `wiremock` test that serves a JSON fixture from `crates/tidal/tests/fixtures/` and asserts the request path and the parsed result. No test touches the real network
- **AC9** — `.github/workflows/ci.yml` runs AC2's checks and the layering script on every push and pull request, on `ubuntu-latest`, installing `libasound2-dev`
- **AC10** — `CLAUDE.md` "Status" and "Build & tooling" are updated from this spec, and the spec's status moves to `implemented`

## Edge cases & errors

- A machine without `libasound2-dev` must still be able to run everything except the ALSA backend (AC7)
- A terminal panic must restore the terminal (raw mode off, alternate screen left) before the panic message prints — a panic hook installed by the binary

## Out of scope (planned follow-up specs)

Numbers are provisional; each is drafted and approved on its own.

- 0002 — Auth: OAuth2 device flow, token storage in the keyring, refresh and recovery when refresh fails
- 0003 — Playback engine: stream resolution (quality ladder), decode, ALSA output, format negotiation, bit-perfect vs. shared output
- 0004 — Queue & playback controls: play/pause/seek/next/prev, shuffle, repeat, auto-advance, volume
- 0005 — Library: favorite tracks/albums/artists, playlists, artist and album pages
- 0006 — Search
- 0007 — Keymap & config: spotify-player-compatible key sequences and `app.toml`/`keymap.toml`
- 0008 — Persistence: last session, volume, device, metadata cache
- 0009 — MPRIS2 / media keys
- 0010 — Mixes & radio

## Facts vs. assumptions

Verified on 2026-10-06:

- Crate versions listed above are the latest published (`cargo search`)
- The dev container has Rust 1.97.0; it does **not** have `libasound2-dev` installed

Assumptions to verify in the spec that depends on them:

- `symphonia` 0.6 can decode every format Tidal serves us — in particular FLAC inside fragmented MP4 from `HI_RES_LOSSLESS` DASH manifests. If not, 0003 decides between a symphonia patch and an optional FFmpeg backend behind a feature
- `keyring` 4 on Linux reaches the Secret Service over D-Bus without native libs, and needs a file fallback when no Secret Service is running (tidalt used an age-encrypted file) — decided in 0002

## Open questions (answer before approval)

1. **Name**: the binary and crate prefix are `tidalt` here. Keep it, or rename (for example to match the repo)?
2. **Daemon/client mode**: tidalt had a headless daemon plus a TUI client over D-Bus. Is that wanted at all? If so it only affects where the boundary between the runtime and `update` sits, which this layout already allows, but it should be on the roadmap
3. **tidalt bugs**: which bugs or behaviours hurt most? They become explicit acceptance criteria in the follow-up specs
