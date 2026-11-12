# 0026 — Realignment: the code, the specs and the rules say the same thing

English | [正體中文](zh-TW/0026-realignment.md)

- **Status**: implemented (2026-11-10; approved the same day: the user asked for a review of the whole project, with leave to revisit any spec or decision)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0001 (implemented: the crates and their rules), 0018 (implemented: CI and releases)
- **User docs**: none (nothing a user sees changes)

## Context

Twenty-five specs were implemented in five weeks. Each was checked against its own text, but nothing checked the project against itself, and it drifted in four ways. The audit of 2026-11-10 (every crate, every spec, `CLAUDE.md`) found:

- **The binary crate had no shape.** `crates/app/src` was 23 flat modules named after the spec that added them: `play` (the settings of every mode, and the `play` command), `playback` (the engine wiring), `player_runtime`, `oneshot` (the `playback` command), `cover` (the TUI's image) beside `covers` (the player's cache), `reserve` beside the audio crate's `reserve`. Spec 0001 splits the program into a player side and a client side; the directory did not
- **Tests buried the code.** `ui.rs` was 861 lines of rendering and 4,657 of tests in one file; `player_runtime.rs` 1,860 and 1,910. `tidal-player-core` already kept long test modules in `<module>/tests.rs`; the other crates did not
- **Specs pointed at things that had moved.** Test plans named files that no longer exist (`crates/core/src/ui.rs`, `crates/app/src/mpris/covers.rs`) and tests that were renamed. "Spec and code agree" was checked by eye at acceptance and never again
- **Spec 0001 described a program that was never built.** It promised a log file through `tracing-appender` (no subscriber was ever installed: the 37 `tracing` calls write nothing, which 0010 and 0025 each rediscovered), said nothing outside the ALSA backend may be Linux-specific (the socket, the lock, systemd and MPRIS are), and said the binary's runtimes hold "no decisions worth testing" (the binary holds about 10,000 lines of tested decisions behind seams)
- **Leftovers.** `tidal_player_api::Client` and `ApiError` were 0001's scaffold, used by one test of themselves; `browse.rs` was mounted with `#[path]`; a refactor had no branch kind, so it could not run CI without becoming a release

None of this is a bug a user can see. All of it makes the next spec slower to write and easier to get wrong.

## Behaviour

Nothing changes for a user: no key, setting, message, file or exit code. Every test that passed before passes after, under its old name.

### The binary's layout

`crates/app/src` follows 0001's split:

```
main.rs          the command line and which mode runs
settings.rs      flags > environment > app.toml > defaults, resolved once
config.rs        the files in the config directory (app.toml, keymap.toml, theme.toml)
panic_hook.rs
player/          the player side: only a process that plays uses it
  runtime.rs       runs core's player::update and executes its effects
  playback.rs      the stream opener and the ALSA engine with its reservation
  http_source.rs   a resolved stream as bytes for the engine
  persist.rs       the remembered playback state (0009)
  cover_cache.rs   the cover cache (0010, 0016)
  hook.rs          the event hook's process side (0025)
  reserve.rs       org.freedesktop.ReserveDevice1 (0003)
  mpris/           the MPRIS adapter (0010)
client.rs        a client's connection to a player, in-process or over the socket
ipc/             the socket: paths, lock, framing, server, client
tui/             the terminal client
  input.rs         key presses to the UI model's actions
  render.rs        the UI model's state to a frame (render/pages.rs, render/theme.rs)
  cover.rs         the album cover's loader and slot (0016)
cli/             the subcommands
  play.rs          `play`, `devices`
  oneshot.rs       `playback …`
  playlist.rs      `playlist …`
  daemon.rs        `daemon`
session/         where the login session is kept (0002)
  store.rs, passphrase.rs, setup.rs, login.rs
```

A module goes where its user is: something only a player does is under `player/`, something only the TUI does under `tui/`. `client` and `ipc` stay at the top because the TUI, the one-shot commands and the in-process player all use them.

### Test modules

A `mod tests` longer than 200 lines lives in `<module>/tests.rs` (for a `mod.rs`, `tests.rs` beside it), in every crate. Shorter ones stay inline. `main.rs` keeps its tests inline whatever their length (a binary root's `mod tests;` would be `src/tests.rs`, away from the code).

### `cargo xtask specs`

Checks every spec's test plan against the code, and runs in CI beside `cargo xtask layering`:

- A table row in `docs/specs/*.md` that names a Rust file of this workspace in backticks (`` `crates/…​.rs` `` or `` `xtask/…​.rs` ``) names a file that exists
- Where the file is followed by `::` and test names in backticks, each name is a function in that file or in its tests module (`<file stem>/tests.rs`; for `mod.rs`, `tests.rs` beside it). So a plan may name the module and keep working when its tests move to `tests.rs`
- Text in parentheses is a note and is not checked (`(renamed from …)`, `(0007's, …)`)
- A failure prints `docs/specs/NNNN-….md:LINE: <what is missing>` for each, and the command exits 1; a clean run prints `specs ok`

It reads the test plans as they are written today; no spec has to change its format for it.

### Branch kinds

`refactor` joins `feat`, `bugfix`, `docs` and `chore`: a change to Rust code that changes no behaviour. A pull request from `refactor/*` runs the CI jobs like `feat/*` and `bugfix/*`, and is **not** a release: no version bump, no tag. The crates it touches are published with the next `feat/*` or `bugfix/*` release, whose bump already covers "crates whose code changed since the last release".

## Acceptance criteria

- **AC1** — `cargo xtask specs` exits 0 and prints `specs ok` when every referenced file and test exists; for each missing file and each missing test it prints `<spec path>:<line>: …` naming it, and exits 1
- **AC2** — A test named after `::` is found in the named file or in that file's tests module; a name inside parentheses is not checked; a workspace path with no `::` is checked for existence only; rows outside tables and paths outside `crates/` and `xtask/` are ignored
- **AC3** — `cargo xtask specs` exits 0 on the real workspace: every test plan of specs 0001–0026 names files and tests that exist
- **AC4** — CI runs `cargo xtask specs` in the `check` job, and a pull request from `refactor/*` runs the jobs: every job's `if` is `github.event_name == 'push' || startsWith(github.head_ref, 'feat/') || startsWith(github.head_ref, 'bugfix/') || startsWith(github.head_ref, 'refactor/')`
- **AC5** — `crates/app/src` has the layout above, and `lib.rs` declares exactly `cli`, `client`, `config`, `ipc`, `panic_hook`, `player`, `session`, `settings`, `tui`
- **AC6** — No `mod tests { … }` outside `main.rs` is longer than 200 lines
- **AC7** — `tidal-player-api` exports no `Client` or `ApiError`, and no module in the workspace is mounted with `#[path]`
- **AC8** — Every test that existed before this spec exists after it with the same name and passes; `cargo test --workspace` runs the same number of tests, less the two that tested the removed scaffold (`ac8_get_json_from_fixture`, `url_has_exactly_one_slash`)
- **AC9** — Spec 0001 describes the program that exists (layout, platform rule, the binary's role, logging, AC8), and `CLAUDE.md` describes the spec format in use, the branch kinds and `cargo xtask specs`

## Edge cases & errors

- **A spec names a test that lives in another file than the one it names, or under a later name** (found 2026-11-10: 0006's page tests had moved to `ui/browse/tests.rs`, 0010's `ac18_cover` was an integration test, 0004 and 0012 named tests that later specs renamed): the plan is corrected; the check is not loosened to search the workspace, because then a plan could name any file
- **A test plan row for a criterion with no test** (`— reviewed at acceptance`): names no file, so nothing is checked
- **A spec quotes a path of another project** (`docs/client-server.md` of spotify-player) or a probe script that was deleted on purpose (`scripts/tidal-*-probe.sh`): not under `crates/` or `xtask/`, not checked
- **A `refactor/*` pull request that does change behaviour**: it is a `feat` or a `bugfix` and is renamed; the kind is a promise the reviewer checks, as with `docs/*`

## Test plan

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `xtask/src/specs.rs` :: `ac1_reports_missing_files_and_tests` (table) | a clean plan gives no findings; a missing file, a missing test and two in one row each give a finding with the spec's line number and the name | stub `check` returns no findings |
| AC2 | `xtask/src/specs.rs` :: `ac2_what_is_read` (table) | tests module fallback (`foo.rs` → `foo/tests.rs`, `mod.rs` → `tests.rs`); parenthesised names skipped; path without `::` checked for existence; prose and foreign paths ignored; several files in one cell each own the names after them | stub `check` returns no findings, so the rows that expect one fail |
| AC3 | `xtask/tests/workspace.rs` :: `ac3_specs_agree_with_the_code` | `cargo xtask specs` exits 0 on the real workspace | the stub command exits 1 |
| AC4 | `xtask/tests/release_workflow.rs` :: `ac8_ci_runs_once_per_commit` (0018's, the `if` changed), `ac4_ci_checks_the_specs` | every job's `if` is the line above; `ci.yml` has a step running `cargo xtask specs` | the workflow has neither |
| AC5 | — reviewed at acceptance | `ls crates/app/src` and `lib.rs` against the layout | — (a move; the compiler and AC8 check it) |
| AC6 | `xtask/tests/workspace.rs` :: `ac6_long_test_modules_have_their_own_file` | no `.rs` file under `crates/` or `xtask/` other than `main.rs` has an inline `mod tests {` block over 200 lines | fails on the pre-move tree (checked at acceptance on `origin/master`) |
| AC7 | `xtask/tests/workspace.rs` :: `ac7_no_path_mounted_modules` | no `#[path` attribute in the workspace's sources | fails on `origin/master` (`crates/api/src/library.rs`) |
| AC8 | — command check | the list of test names from `cargo test --workspace -- --list` before and after differs only by module path, less the two removed | — |
| AC9 | — reviewed at acceptance | 0001 and `CLAUDE.md` against the code | — |

AC5–AC7 were moves made before their tests could be written first (a move has no red state of its own: the tree before it is the red state). Their tests are checked against `origin/master` at acceptance instead.

## Crate placement

- `xtask/src/specs.rs`: the check, pure over `(spec path, text)` and a function that reads a workspace file; `main.rs` gains the `specs` command
- Everything else is a move inside its crate; no crate gains or loses a dependency

## Facts vs. assumptions

Verified on 2026-11-10:

- `cargo test --workspace`, clippy with `-D warnings` and `cargo xtask layering` are clean on `origin/master` (7770837) before any change
- No `tracing` subscriber is installed: `tracing-subscriber` is not in `Cargo.lock`
- `tidal_player_api::Client` is used only by `crates/api/tests/harness.rs` and its own unit test
- `insta` names a snapshot after the test's module path and keeps it in `snapshots/` beside the test's file, so a moved test needs its snapshots moved and renamed; their contents are unchanged

## Decisions

1. **Spec 0001 is corrected, not obeyed, where the program outgrew it.** Its platform rule becomes "Linux only; the ALSA backend is the only code that needs ALSA", and its claim about the binary becomes the rule the code follows: decisions are pure functions, I/O sits behind a seam
2. **Logging stays undone, and 0001 now says so.** Installing a subscriber is a feature (where the file is, what a client shows) that issue #64 (the logs page) has to design; 0001 no longer promises it
3. **`cargo xtask specs` is lenient about the tests module and strict about the file.** A plan written against `foo.rs` survives the move of its tests to `foo/tests.rs`; a plan naming the wrong file is an error
4. **`refactor/*` runs CI and is not a release.** The alternative (`chore/*`) skips CI, which a 20,000-line move cannot do; the other (`feat/*`) would publish five crates for no user-visible change
5. **Pure decisions stay in the binary crate.** Moving the MPRIS model, the settings and the save schedule into `tidal-player-core` would make core's rule ("no I/O") truer on paper and its API larger for nobody: nothing but the binary uses them. `core` keeps what crosses the player/client boundary

## Out of scope

- A log file or `tracing` subscriber (issue #64)
- Splitting `player/runtime.rs` (1,860 lines) and `core::ui` further: their size is one state machine each, and no boundary inside them is as clear as the ones moved here
- The slow tests of the binary crate's unit suite (about 130 s, most of it real sleeps in the hook and cover tests)
- Renaming anything a user or a script sees
