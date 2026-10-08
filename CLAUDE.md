# CLAUDE.md

**tidal-player**: a terminal Tidal player modelled on [spotify-player](https://github.com/aome510/spotify-player). This is a from-scratch rebuild of [tidalt](https://github.com/PEKKA1117/tidalt), which grew too buggy to keep patching. Nothing is carried over as code; behaviour worth keeping is re-specified and re-tested here first.

## Status

Rust workspace, scaffolded by `docs/specs/0001-architecture.md` (implemented). That spec fixes the crate layout, the dependency rules, the player/client boundary (`Command`/`Event` in `tidal_player_core::protocol`) and the run modes. Spec `0002-auth.md` (implemented) adds the device-flow login, session storage (keyring, else an age-encrypted file) and refresh/recovery; user docs in `docs/login.md`. Spec `0004-queue-and-controls.md` (implemented) adds the player state machine (`tidal_player_core::player`), the queue, playback controls, autoplay, the player runtime and the first TUI screen; user docs in `docs/tui.md` and `docs/playback.md`. Spec `0005-daemon-and-clients.md` (implemented) adds the headless daemon (systemd user service), one player per user over a Unix socket (`tidal-player::ipc`), TUI clients that attach to it, one-shot `playback` commands, and releasing the audio device while paused; user docs in `docs/daemon.md`. Spec `0006-library.md` (implemented) adds the library: pages with a history (favorites, playlists, albums, artists), windows that load as you scroll, play/queue from a page, the actions popup, favorites and playlist editing, answered by the player over the same socket; user docs in `docs/tui.md` and `docs/playback.md`. Spec `0007-search.md` (implemented) adds the search page (`g s`): a query, Tidal's top hit and four result windows that load as you scroll, answered by the player; user docs in `docs/tui.md` and `docs/playback.md`. Follow-up specs are listed in the "Out of scope" sections.

## Build & tooling

Rust toolchain pinned in `rust-toolchain.toml` (1.97, edition 2024). Run from the repo root:

- **Format**: `cargo fmt --all` after editing any `.rs` file (CI: `cargo fmt --all --check`)
- **Lint**: `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo xtask layering` (crate dependency rules) before finishing a task; fix everything they report
- **Build**: `cargo build --workspace`
- **Test**: `cargo test --workspace`; one crate: `cargo test -p <crate>` (`tidal-player-core`, `tidal-player-api`, `tidal-player-audio`, `tidal-player`, `xtask`); one test: `cargo test -p <crate> <name>`. Snapshot (`insta`) changes are reviewed, never blindly accepted: read the `.snap.new`, then accept it (`INSTA_UPDATE=always`, or `cargo insta accept`). Never commit `.snap.new`
- **System deps**: `pkg-config` and `libasound2-dev` (only for the default `alsa` feature of `tidal-player-audio`). `cargo test -p tidal-player-audio --no-default-features` works without them
- **Dependencies**: every third-party crate is declared once in the root `[workspace.dependencies]`; crates use `dep = { workspace = true }`. Merges regenerate `Cargo.lock` with cargo, never by hand

## Development approach: tech-lead

The primary session works as a **tech-lead**, following the `tech-lead` skill vendored at `.claude/skills/tech-lead/SKILL.md` (from [akunzai/agent-skills](https://github.com/akunzai/agent-skills/blob/main/skills/tech-lead/SKILL.md) @ `50117a94d9d7`, MIT, see its `LICENSE`). Read it, and its `references/brief-elements.md`, before starting any non-trivial feature or fix:

- Stay in the primary session for a few-line or single-file mechanical edit and for architecture decisions; delegate anything larger to implementer subagents
- Slice the work (files each slice may touch, parallel/serial, durability), confirm parallelism and its cap with the user, isolate each slice in its own git worktree and branch, brief one implementer per slice, then accept, integrate and clean up in the primary session
- Tests, builds and lint run in the primary session, which reads their output itself

It combines with SDD + TDD below as follows:

- The spec is written (or updated) and **approved by the user in the primary session before slicing**. Slices map to the spec's acceptance criteria; every criterion belongs to exactly one slice
- Every brief quotes the spec (or links it and quotes the criteria the slice owns), and splits verified facts from assumptions
- Every brief's acceptance requires the red → green → refactor sequence, the failing-test output from the red step, and a clean run of every "Build & tooling" check
- At acceptance the tech-lead verifies that the test was written first and fails without the change (revert the non-test part, or check out the red commit, and run it), that each owned criterion has a test, and that spec and code agree. A mismatch goes back to the same implementer

## Development workflow: SDD + TDD

Every feature and every bug fix, however small, follows spec-driven development (SDD) and then test-driven development (TDD), in this order:

1. **Spec first (SDD)** — before touching code, write or update the spec:
   - Features: a numbered spec in `docs/specs/` (`NNNN-short-slug.md`), with the sections in "Spec format" below
   - Bug fixes: add a "Bugs" entry to the spec that covers the area — expected vs. actual behaviour, root cause, and the new acceptance criterion — and correct the spec if it was wrong or silent on the case. No covering spec means the area was never specified: write one
   - User-facing behaviour (keys, config, layout) is also documented in `docs/` pages for users; the spec links to them rather than duplicating them
2. **Red (TDD)** — write the failing tests named in the spec's test plan (for a bug: a test that reproduces it). Run it and confirm it fails **for the expected reason**, not on a compile error or a missing fixture. Commit the red state on its own (`test: …`) so acceptance can check it out
3. **Green** — write the minimal code that makes the tests pass
4. **Refactor** — clean up with the tests green, then run every "Build & tooling" check

Rules:

- Tests are table-driven where it fits and live next to the code they cover
- Code that can't be unit-tested directly (audio output, native libraries, D-Bus/MPRIS, the live Tidal API) sits behind a seam: keep the pure logic (format choice, fallback decisions, parsing, request building, state transitions) in testable functions, and fake the boundary. Tidal API tests run against recorded fixtures, never the network
- UI state is a pure model: key → action → new state is unit-tested without a terminal; rendering is tested with snapshot/golden tests at fixed sizes
- Never skip, disable or weaken a test to get green. If a spec change makes a test obsolete, update the test to the new spec in the same change
- A bug fix without a regression test is not done

### Spec format

Each `docs/specs/NNNN-*.md` has:

- **Status**: `draft` → `approved` (by the user) → `implemented`. Only `approved` specs are sliced
- **Context**: what problem this solves; what tidalt did here and what went wrong, if relevant
- **Behaviour**: what the user sees and does — inputs/outputs, keys, config, states
- **Acceptance criteria**: numbered (`AC1`, `AC2`, …), each observable and testable; tests reference them by number in their name or comment
- **Edge cases & errors**: network loss, expired auth, empty lists, unsupported formats, tiny terminals, …
- **Test plan**: a table mapping every AC to the test that proves it (file :: test name, what it asserts, the expected red failure), or, for a criterion no automated test can cover, how it is checked at acceptance and why it has no test. A spec without a test plan can't be approved
- **Out of scope**
- **Bugs** (added over time): see step 1

## Definition of done

A change is done when: the spec is `approved` (or `implemented`) and matches the code; every acceptance criterion it touches has a passing test; every "Build & tooling" check is clean; `README.md` is up to date (see "Conventions"); and the commit history shows the red commit before the green one.

## Conventions

- Commits follow Conventional Commits (`feat(ui): …`, `fix(player): …`, `test: …`, `docs(spec): …`)
- One spec → one PR where practical; a PR description lists the spec and the acceptance criteria it covers
- Every PR updates `README.md` so it matches the PR's result: what works, how to use it, links to the user docs. A PR that changes nothing user-visible still checks it and says so in its description
- Keep this file for rules and stable facts. Implementation detail belongs in specs and code comments — tidalt's CLAUDE.md turned into a per-file changelog that went stale

## Carried over from tidalt (facts to re-verify, not requirements)

These cost time in tidalt. Re-check each against the live API or hardware before a spec relies on it.

- Tidal auth uses the OAuth2 device flow; sessions need refresh-token handling and a recovery path when the refresh fails
- Daily Mixes: the v2 `openapi.tidal.com/v2/userRecommendations` resource was removed and 404s. tidalt used v1 `GET /v1/pages/my_collection_my_mixes` for the list and `GET /v1/mixes/{mixId}/items` for tracks; video mixes and non-`track` items must be filtered out
- Daemon-client mode (kept, see spec 0001 "Run modes"): one binary ran standalone, as a headless daemon (systemd user service, controllable by MPRIS clients such as `playerctl`), or as a TUI client of that daemon over D-Bus. The daemon held the audio device only while playing and released it on pause so other apps could use it
- Stream quality ladder: `HI_RES_LOSSLESS` → `LOSSLESS` → `HIGH` → `LOW`
- Bit-perfect output meant opening ALSA `hw:` directly, negotiating the PCM format per device (some DACs advertise a broken `S16_LE` endpoint, so 16-bit sources preferred `S32_LE` first), asking PipeWire to release the card via `org.freedesktop.ReserveDevice1`, and falling back to `plughw:` only when format negotiation is refused — never on a busy device
