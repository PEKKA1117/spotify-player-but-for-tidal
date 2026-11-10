# 0001 — Architecture & project scaffold

- **Status**: implemented (2026-10-06; amended 2026-11-10 by [0026](0026-realignment.md): the platform rule, the binary's layout and role, logging, AC8 and AC9 now describe the program as built)
- **Owner**: tech-lead (primary session)

## Context

tidalt (Go + CGO + FFmpeg + ALSA) grew too buggy to keep patching. Its problems were structural as much as local:

- UI state, I/O and playback were tangled in one large BubbleTea model, so most behaviour could only be tested by driving the whole UI
- CGO + a dynamically or statically linked FFmpeg made builds, CI and packaging fragile
- `CLAUDE.md` became a per-file changelog instead of a spec, so intended behaviour lived nowhere testable

This spec picks the stack and the crate layout, and sets up an empty workspace in which every later spec can be built test-first. It adds **no user-facing features** beyond `--help`/`--version` and an empty TUI frame.

## Decisions

### Name

The binary, the crate prefix and the config/data directory names are **`tidal-player`** (`~/.config/tidal-player/`, `~/.local/state/tidal-player/`), in honour of the original tidalt author. "tidalt" in this spec always means the old Go project.

### Language & toolchain

- Rust, edition 2024, stable toolchain pinned in `rust-toolchain.toml` (currently 1.97), with `rustfmt` and `clippy` components
- **Linux only**, as tidalt was: ALSA output, a Unix socket and lock file between player and clients (0005), systemd for the daemon, the session bus for MPRIS (0010) and the device reservation (0003). The ALSA backend is the only code that needs the ALSA library, behind the `alsa` feature (AC7); the rest builds on any Unix. Windows is issue #21, not a constraint on today's code

### Workspace layout

```
Cargo.toml                 # [workspace], shared [workspace.dependencies] and [workspace.lints]
rust-toolchain.toml
crates/
  core/    -> tidal-player-core   # domain types, protocol, player + UI state machines, keymap. NO I/O.
  api/     -> tidal-player-api    # Tidal HTTP API client; maps API DTOs into core types
  audio/   -> tidal-player-audio  # decode + output engine; Sink trait, ALSA backend behind a feature
  app/     -> tidal-player        # the binary: CLI, player runtime (in-process or daemon), TUI client, rendering
                                  #   src/ is split by side: player/, tui/, cli/, session/, ipc/ (see 0026 "The binary's layout")
xtask/     -> xtask        # dev-only tooling; `cargo xtask layering` enforces the dependency rules below,
                           #   `cargo xtask specs` checks every spec's test plan against the code (0026)
.cargo/config.toml         # `xtask` alias
.github/workflows/ci.yml
```

Allowed dependency edges (everything else is forbidden):

| crate                | may depend on                         |
|----------------------|---------------------------------------|
| `tidal-player-core`  | — (only pure libs: serde, thiserror…) |
| `tidal-player-api`   | `tidal-player-core`                   |
| `tidal-player-audio` | — (not `core`, not `api`)             |
| `tidal-player`       | all of the above                      |
| `xtask`              | none of the above (dev tooling only)  |

### Application pattern (the main testability fix)

Elm-style, as a pure core with an imperative shell, split along the **player/client boundary** so daemon-client mode (kept from tidalt) is built in from the start rather than bolted on:

- **Player side** — `tidal_player_core::player`: `PlayerState` (queue, current track, position, volume, shuffle/repeat, output device), `PlayerInput` (a client `Command`, an audio-engine event, an API result, a tick) and `PlayerEffect` (resolve stream, start/stop/seek the engine, persist, broadcast an `Event`). `fn update(&mut PlayerState, PlayerInput) -> Vec<PlayerEffect>`
- **Client side** — `tidal_player_core::ui`: `State` (pages, cursors, overlays, the client's last-known copy of the player state), `Action` (key presses already resolved through the keymap, an `Event` from the player, API results for browsing, ticks) and `Effect` (send a `Command`, fetch a page of data, quit the client). `fn update(&mut State, Action) -> Vec<Effect>`
- **Protocol** — `tidal_player_core::protocol`: `Command` (client → player: play, pause, next, seek, queue edits, volume, …) and `Event` (player → clients: state snapshots and changes). Both derive `Serialize`/`Deserialize`, because in daemon mode they cross a process boundary. The client never touches the player's state directly: everything goes through `Command`/`Event`
- Both `update` functions are pure and synchronous. All behaviour that can be decided without I/O is decided there and unit-tested there
- The binary's runtimes turn terminal events, socket messages, API results and engine events into inputs, call `update`, and execute the returned effects
- What only one process decides stays in the binary, by the same pattern: the decision is a pure function (which settings win, when to save, what MPRIS shows, which cover files to delete) and the I/O it needs sits behind a small trait with a fake. `tidal-player-core` holds what crosses the player/client boundary (the protocol, the two state machines, the domain types); it is not a place for every pure function (0026, decision 5)
- Rendering is `fn render(&State, &mut Frame)`: a pure function of the client state, tested with ratatui's `TestBackend` + `insta` snapshots at fixed sizes

### Run modes

One binary, three modes, the same player code in each (as in tidalt):

- **Standalone** (`tidal-player`, no daemon running) — player runtime and TUI client in one process, joined by an in-memory channel carrying `Command`/`Event`
- **Daemon** (`tidal-player daemon`) — the player runtime alone, headless, accepting clients over a local transport; meant to run as a systemd user service
- **Client** (`tidal-player` when a daemon is running) — the TUI client alone, attached to the daemon over the transport

The transport (a Unix socket carrying JSON lines; tidalt used D-Bus), attach/detach behaviour, multiple clients, one-shot CLI commands (`tidal-player playback next`, as spotify-player has) and releasing the audio device while paused are decided in [0005](0005-daemon-and-clients.md). This spec only fixes the boundary: the `Command`/`Event` types and the rule that clients reach the player through them alone.

### Concurrency

- `tokio` multi-thread runtime in the binary for HTTP and the event loop
- Audio decode + output run on a **dedicated OS thread** owned by `tidal-player-audio` (never on the async runtime), controlled through a command channel and reporting through an event channel. `tidal-player-audio` exposes a synchronous API and does not depend on tokio

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
| MPRIS ([0010](0010-mpris.md), implemented) | `zbus` 5 (plain; `mpris-server` not used) |
| errors             | `thiserror` in libraries, `anyhow` in the binary |
| logging            | `tracing` macros at the call sites; a subscriber keeps the lines in a buffer, in `tidal-player.log` in the state directory and, for the daemon, on stderr, and the logs page shows them (see [0027](0027-logs-page.md)). What a user must see goes in the player's message row or on stderr, never only in a log |
| tests              | `insta` (snapshots), `wiremock` (HTTP fixtures), `assert_cmd` (CLI) |

### Build & tooling (copied into `CLAUDE.md` when this spec is approved)

- **Format**: `cargo fmt --all` (CI: `cargo fmt --all --check`)
- **Lint**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`, plus `cargo xtask layering` and `cargo xtask specs` (0026)
- **Build**: `cargo build --workspace`
- **Test**: `cargo test --workspace`; one crate: `cargo test -p tidal-player-core`; one test: `cargo test -p tidal-player-core <name>`. Snapshot changes are reviewed, never blindly accepted (`cargo insta review`, or inspect the `.snap.new` files)
- **System deps**: `pkg-config`, `libasound2-dev` (only for the `alsa` feature). `cargo test -p tidal-player-audio --no-default-features` builds without them

## Acceptance criteria

- **AC1** — On a clean checkout with the system deps installed, `cargo build --workspace` succeeds and produces a `tidal-player` binary
- **AC2** — `cargo fmt --all --check`, the clippy command above, and `cargo test --workspace` all pass
- **AC3** — `cargo xtask layering` exits 0 on the scaffold and non-zero, naming each offending edge, when any edge outside the table is added, or when `tidal-player-core` gains any of `tokio`, `reqwest`, `crossterm`, `ratatui`, `alsa`, `symphonia`, `zbus`, `keyring`, `serde_json` as a normal (non-dev) dependency, directly or transitively
- **AC4** — `tidal-player --version` prints `tidal-player <version>` and exits 0; `tidal-player --help` exits 0 (tested with `assert_cmd`)
- **AC5** — `tidal_player_core::ui` exposes `State`, `Action`, `Effect` and `update`; `update(&mut State::default(), Action::Quit)` returns `[Effect::Quit]`, and any other action on the default state returns no effects (unit test)
- **AC6** — `render` of `State::default()` into an 80×24 `TestBackend` matches a committed `insta` snapshot showing an empty layout with the app name
- **AC7** — `tidal-player-audio` defines a `Sink` trait with an in-memory test sink; the ALSA backend is behind a default-on `alsa` feature, and `cargo test -p tidal-player-audio --no-default-features` passes without `libasound2-dev`
- **AC8** — `tidal-player-api` takes its base URLs from the caller, and its tests serve JSON fixtures from `crates/api/tests/fixtures/` with `wiremock`, asserting the request path and the parsed result. No test touches the real network. (The scaffold's own `Client` and its test were removed by 0026: every request goes through `auth::Authenticator`)
- **AC9** — `.github/workflows/ci.yml` runs AC2's checks and `cargo xtask layering` on `ubuntu-latest`, installing `libasound2-dev`, once per commit: on every push to `master` and on pull requests from `feat/*`, `bugfix/*` and `refactor/*` branches (0026). A push to any other branch or a tag runs nothing, and pull requests from `docs/*` and `chore/*` skip the jobs (the release waits for `master`'s run instead of a tag's, spec 0018 Bug 3)
- **AC10** — A panic while the TUI is running restores the terminal (raw mode off, alternate screen left) **before** the panic message is printed, via a panic hook the binary installs
- **AC11** — `tidal_player_core::protocol` defines `Command` and `Event` deriving `Serialize`/`Deserialize`, with at least `Command::Shutdown` and `Event::ShuttingDown`; every variant survives a JSON round-trip unchanged, and `tidal-player-core` has no dependency on any transport crate (covered by AC3's forbidden list)
- **AC12** — `CLAUDE.md` "Status" and "Build & tooling" are updated from this spec, and the spec's status moves to `implemented`

## Test plan

Each automated test is named after the criterion it proves (`ac5_…`). "Red" is the failure the implementer must show before writing the code: a failing assertion, never a compile error. So the red commit already contains the stub types and functions the test calls, with stub bodies (`todo!()` is not allowed: it panics instead of failing the assertion).

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | — command check | `cargo build --workspace` exits 0 in CI, and AC4's tests run the built `tidal-player` binary | — |
| AC2 | — command check | `cargo fmt --all --check`, clippy (`-D warnings`) and `cargo test --workspace` exit 0 | — |
| AC3 | `xtask/src/layering.rs` :: `ac3_rules` (table-driven, over a hand-built dependency graph) | allowed graph → no violations; `core → tokio` → violation `tidal-player-core -> tokio`; `core → serde_x → tokio` (transitive) → violation; `audio → core`, `api → audio`, `core → api` → one violation each, naming the edge; `core` with `tokio` as a dev-dependency only → no violation | stub `check()` returns no violations, so every violating row fails |
| AC3 | `xtask/tests/workspace.rs` :: `ac3_real_workspace_is_clean` | `cargo xtask layering` on the real workspace exits 0 | the stub command exits 1 |
| AC4 | `crates/app/tests/cli.rs` :: `ac4_version` | `tidal-player --version` stdout is exactly `tidal-player <CARGO_PKG_VERSION>\n`, exit 0 | stub `main` prints nothing |
| AC4 | `crates/app/tests/cli.rs` :: `ac4_help` | `tidal-player --help` exits 0 and stdout contains `Usage:` | stub `main` prints nothing |
| AC5 | `crates/core/src/ui/mod.rs` :: `ac5_quit_emits_quit` | `update(&mut State::default(), Action::Quit) == vec![Effect::Quit]` | stub `update` returns `vec![]` |
| AC5 | `crates/core/src/ui/mod.rs` :: `ac5_other_actions_emit_nothing` (table over every non-`Quit` variant, `Action::Tick` at minimum) | each returns `vec![]` and leaves `State` equal to the default | a second red/green cycle after `ac5_quit_emits_quit` is green: the simplest code that passes the first test (`vec![Effect::Quit]` for every action) fails this one |
| AC6 | `crates/app/src/tui/render.rs` :: `ac6_empty_state_80x24` | the 80×24 `TestBackend` buffer contains `tidal-player`, **and** matches the committed `insta` snapshot. The tech-lead reviews the snapshot by eye at acceptance | stub `render` draws nothing, so the `contains` assertion fails (a missing snapshot alone does not count as red) |
| AC7 | `crates/audio/src/sink.rs` :: `ac7_memory_sink_records_samples` | after `open(format)` and two `write` calls, `MemorySink` reports that format and the two buffers concatenated, in order | stub `write` is a no-op |
| AC7 | — CI job `audio-no-alsa` | `cargo test -p tidal-player-audio --no-default-features` passes on a runner **without** `libasound2-dev` | — |
| AC8 | `crates/api/tests/metadata.rs` :: `ac17_get_track` (0004's; the scaffold's `ac8_get_json_from_fixture` was removed with its `Client`, 0026) | `wiremock` serves a recorded fixture at the track's path; the client built with the mock server's URL returns the parsed `Track` | — (the scaffold's red was a stub `get_json` returning an error) |
| AC9 | — reviewed at acceptance; the triggers and the jobs' `if` are tested by spec 0018 AC8 (`xtask/tests/release_workflow.rs` :: `ac8_ci_runs_once_per_commit`) | the workflow file has the AC2 steps, `cargo xtask layering` and the `audio-no-alsa` job, and the PR's own CI run is green | — |
| AC10 | `crates/app/tests/panic_hook.rs` :: `ac10_restore_runs_before_report` (its own test binary, because panic hooks are process-global) | with a fake `restore` and a fake previous hook that each append to a shared log, `catch_unwind(\|\| panic!())` leaves the log as `["restore", "report"]` | stub `install_panic_hook` does nothing, so only the fake previous hook runs and the log is `["report"]` |
| AC11 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (table over every `Command` and `Event` variant; `serde_json` as a dev-dependency only) | `from_str(&to_string(&v)) == v` for each | stub `Serialize` impl writes `null` (hand-written, replaced by `#[derive]` in green), so deserialising back fails |
| AC12 | — reviewed at acceptance | `CLAUDE.md` matches this spec; status is `implemented` | — |

Not covered by automated tests, on purpose: real terminal raw-mode handling and real ALSA output. They sit behind the `restore` and `Sink` seams above, and are exercised manually only when a later spec adds behaviour that depends on them.

## Edge cases & errors

- A machine without `libasound2-dev` must still be able to run everything except the ALSA backend (AC7)
- A panic while the terminal is in raw mode (AC10)

## Out of scope

Everything a user does. The follow-up specs this one planned (auth, the playback engine, the queue, the daemon, the library, search, the keymap and config, persistence, MPRIS, mixes and radio) are implemented as 0002–0011; `CLAUDE.md` "Features" lists every spec since.

## Facts vs. assumptions

Verified on 2026-10-06:

- Crate versions listed above are the latest published (`cargo search`)
- The dev container has Rust 1.97.0; it does **not** have `libasound2-dev` installed

Assumptions to verify in the spec that depends on them:

- `symphonia` 0.6 can decode every format Tidal serves us — in particular FLAC inside fragmented MP4 from `HI_RES_LOSSLESS` DASH manifests. If not, 0003 decides between a symphonia patch and an optional FFmpeg backend behind a feature
- `keyring` 4 on Linux reaches the Secret Service over D-Bus without native libs, and needs a file fallback when no Secret Service is running (tidalt used an age-encrypted file) — decided in 0002

## Bugs

- **Test plan error, AC10 red (found at acceptance).** The plan said the stubbed hook leaves the log as `[]`. Actually the fake previous hook still runs, so the log is `["report"]`. The test was right; the plan row is corrected.
