# CLAUDE.md

**tidal-player**: a terminal Tidal player modelled on [spotify-player](https://github.com/aome510/spotify-player). This is a from-scratch rebuild of [tidalt](https://github.com/PEKKA1117/tidalt), which grew too buggy to keep patching. Nothing is carried over as code; behaviour worth keeping is re-specified and re-tested here first.

## Easter-egg specs

`docs/Eastereggs/` holds spec parts kept out of the repository: excluded in `.git/info/exclude`, never committed, present only in the maintainer's checkout (absolute path `/home/df/claude/spotify-player-but-for-tidal/docs/Eastereggs/`, also from a worktree). A spec that has one starts with a line `@docs/Eastereggs/<hash>.md` (a path from the repository root). That file is part of the spec, with the same authority:

- Read it before slicing, implementing or accepting anything the spec covers; its criteria are owned by slices and accepted like the spec's
- Where it and the spec disagree, it wins: the tracked spec keeps its text (so it gives nothing away) and the implementation, tests and examples follow the Easter-egg file. The file lists the parts of the spec it replaces
- Briefs quote it as they quote the spec
- Never describe what it says in tracked prose (specs, user docs, `README.md`, this file, commit messages, PR and issue text); the tracked spec only names the file
- A spec naming a file that is missing (another clone, CI): say so and stop rather than guessing

## Status

Rust workspace (toolchain and checks under "Build & tooling"). The features below exist today. Follow-up specs are listed in each spec's "Out of scope" section.

## Features

One entry per implemented spec: an introduction to the feature, so a reader new to the project knows what the player can do and where to read more. When a spec is implemented, its implementer adds its entry here (or extends the entry of the feature it grows): what the feature is and does for the user, in a few sentences, plus links to the spec and the user docs. It is not a record of the work: no changelog of what was added, changed or fixed, no file-by-file notes, no AC numbers. Those live in the spec, the commits and the PR.

- **Architecture** ([0001](docs/specs/0001-architecture.md)): a Cargo workspace of four crates (`tidal-player-core` for pure logic and UI state, `tidal-player-api` for Tidal, `tidal-player-audio` for decoding and output, the `tidal-player` binary), with dependency rules checked by `cargo xtask layering`. The player and its clients talk only through `Command`/`Event` (`tidal_player_core::protocol`), so the same player runs in the TUI's process or behind a daemon
- **Login** ([0002](docs/specs/0002-auth.md), [docs/login.md](docs/login.md)): sign in with Tidal's device flow (open a link, enter a code). The session is kept in the system keyring, or in an age-encrypted file when there is none; tokens refresh on their own and are saved, and when a refresh fails the player says so and lets you log in again without restarting
- **Playback engine** ([0003](docs/specs/0003-playback-engine.md), [docs/playback.md](docs/playback.md)): resolves a track to the best stream Tidal offers down the quality ladder, decodes it, and plays it bit-perfect on an ALSA `hw:` device, taking the card from PipeWire and falling back to `plughw:` only when the device refuses the format. Next tracks are preloaded for gapless playback
- **Queue and controls** ([0004](docs/specs/0004-queue-and-controls.md), [docs/tui.md](docs/tui.md), [docs/playback.md](docs/playback.md)): a play queue with play/pause, seek, next/previous, shuffle, repeat, volume and gapless auto-advance; autoplay continues with tracks Tidal suggests when the queue runs out. The TUI shows a now-playing bar and the queue, filled from Tidal links or IDs
- **Daemon and clients** ([0005](docs/specs/0005-daemon-and-clients.md), [docs/daemon.md](docs/daemon.md)): `tidal-player daemon` runs the player headless as a systemd user service, one per user, over a Unix socket. Any number of TUIs attach to it, `tidal-player playback …` sends one-shot commands, and the audio device is released while paused so other apps can use it
- **Library** ([0006](docs/specs/0006-library.md), [docs/tui.md](docs/tui.md)): browse your favorite tracks, playlists, albums and artists, and the album, playlist and artist pages they lead to, with a page history and lists that load as you scroll. Play a track with the rest of its list queued, add to the queue, and use the actions popup to edit favorites and your playlists
- **Search** ([0007](docs/specs/0007-search.md), [docs/tui.md](docs/tui.md)): the search page (`g s`) shows Tidal's top hit and matching tracks, albums, artists and playlists in four windows that load as you scroll; their rows play, queue, open and take actions like any other page's
- **Filtering a list** ([0012](docs/specs/0012-filter.md), [docs/tui.md](docs/tui.md#filtering-a-list)): `/` filters the focused window (any list, the search results included) or the queue as you type: only the rows with a field containing the text, ignoring case, stay, and the title shows how many match. `enter` keeps the filter and `esc` clears it; `enter` on a track plays the matching tracks, and a list that loads as you scroll fetches more pages until enough rows match
- **Mixes and radio** ([0011](docs/specs/0011-mixes-and-radio.md), [docs/tui.md](docs/tui.md#mixes-and-radio)): `g m` opens the mixes Tidal builds for you (daily mixes, *My Daily Discovery*), and a mix opens as a track list like an album's. `r` on a track or an artist (or *Go to radio* in the actions popup) opens its radio, up to 100 tracks Tidal suggests. Mix and radio pages are ordinary pages: nothing plays or changes in the queue until `Enter` or `Z` on a track
- **Resuming the last session** ([0009](docs/specs/0009-persistence.md), [docs/playback.md](docs/playback.md#resuming-the-last-session)): the player (standalone or daemon) remembers its queue, position, shuffle, repeat, autoplay and volume in the state directory and restores them, stopped, when it starts again, so a restart or reboot picks up where you left off without playing until you press play. `remember_playback = false` turns it off; `logout` forgets it
- **Config, keymap and keys help** ([0008](docs/specs/0008-keymap-and-config.md), [docs/config.md](docs/config.md)): settings in `~/.config/tidal-player/app.toml` (flags and environment still win) and keys in a spotify-player-compatible `keymap.toml`; `examples/` holds the defaults. `?` opens the keys help: the keys for where you are first, filterable, and `Enter` runs one
- **Key hints** ([0013](docs/specs/0013-key-hints.md), [docs/tui.md](docs/tui.md#key-hints)): press the first key of a sequence (`g`) and wait, and a box docked at the bottom of the page lists the keys that can follow and what each does there, with the keys help's texts and dimming; nested prefixes show how many bindings they lead to. It never changes what keys do; `key_hints` and `key_hints_delay_ms` (default 1 s) in `app.toml` turn it off or change the wait
- **Desktop controls and media keys** ([0010](docs/specs/0010-mpris.md), [docs/mpris.md](docs/mpris.md)): every player (standalone, daemon, `play`) publishes itself on the session bus as the MPRIS2 player `org.mpris.MediaPlayer2.tidal_player`, so GNOME/KDE media controls, the keyboard's media keys, Bluetooth headsets and `playerctl` show and control it, album cover included (kept in a small cover cache, `max_cover_arts`). `Play`, `Pause`, `Stop` and the shuffle/repeat setters are also one-shot commands. No session bus, or `mpris = false`: the player runs as before
- **Output device selection** ([0014](docs/specs/0014-device-selection.md), [docs/tui.md](docs/tui.md#output-device), [docs/playback.md](docs/playback.md#choosing-the-output-device)): `D` opens the Devices popup, the player's output devices read fresh from its machine with `●` on the one in use, and `Enter` moves playback there; `tidal-player playback device [NAME]` lists or switches from a shell. A playing track moves at its position with nothing lost, and a device that will not open leaves the player on the previous one with a message. The choice is the player's, shown by every client, and lasts for the run: `output_device` makes it permanent
- **Album cover** ([0016](docs/specs/0016-cover-art.md), [docs/tui.md](docs/tui.md#album-cover)): in a terminal that draws images (kitty, sixel or iTerm2, detected at start or forced with `cover_img_protocol`, through tmux too, or `halfblocks` anywhere) the TUI draws the current track's album cover left of a taller playback window (`cover_img_rows`, default 6), which keeps its height whatever plays. The TUI never downloads: it reads the cover the player (standalone or daemon) keeps in its cover cache, now kept whether MPRIS is on or not. `cover_img = false` gives the plain 4-row window and sends the terminal nothing
- **Themes and colours** ([0017](docs/specs/0017-themes.md), [docs/config.md](docs/config.md#themetoml), [docs/tui.md](docs/tui.md#themes)): the TUI can be coloured with a spotify-player-compatible `theme.toml` (palette and per-component styles). `T` opens the Themes popup, which previews each theme as the cursor moves; `theme` in `app.toml` (or `-t`) picks the starting one. The built-in `default` theme is the TUI's look without themes, and a theme leaves a component it does not style as it was; `examples/theme.toml` holds spotify-player's look and some of its sample themes to copy. Only the TUI reads `theme.toml`, so a typo in a colour never stops the music
- **Confirming removals** ([0019](docs/specs/0019-confirm-removals.md), [docs/tui.md](docs/tui.md#confirming-removals)): *Remove from favorites*, *Remove from this playlist* and *Remove from queue* ask a `y/n` question (`Remove <title> from the queue? (y/n)`) before they act, from the actions popup, an `[[actions]]` binding or `d d`, which removes the row under the cursor from the queue, a playlist of yours or your favorites (and deletes a playlist of yours in the library, always asking), so that `d` alone stays free for removing a selection later. `n` or `Esc` sends nothing, and the question holds what to remove, not the cursor. `confirm_removals = false` in `app.toml` removes at once; *Delete playlist* asks either way
- **User docs on the wiki** ([0015](docs/specs/0015-wiki.md)): the user docs are published to the [GitHub wiki](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki), in English and 正體中文, with a home page and sidebar; README, the daemon's unit and error messages send readers there. `docs/` stays the source: `cargo xtask wiki <dir>` builds the pages (doc links become wiki links, other repository links `blob/master` URLs), and the `Wiki` workflow publishes them on every push to `master` that touches `docs/`

## Build & tooling

Rust toolchain pinned in `rust-toolchain.toml` (1.97, edition 2024). Run from the repo root:

- **Format**: `cargo fmt --all` after editing any `.rs` file (CI: `cargo fmt --all --check`)
- **Lint**: `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo xtask layering` (crate dependency rules) before finishing a task; fix everything they report
- **Build**: `cargo build --workspace`
- **Test**: `cargo test --workspace`; one crate: `cargo test -p <crate>` (`tidal-player-core`, `tidal-player-api`, `tidal-player-audio`, `tidal-player`, `xtask`); one test: `cargo test -p <crate> <name>`. Snapshot (`insta`) changes are reviewed, never blindly accepted: read the `.snap.new`, then accept it (`INSTA_UPDATE=always`, or `cargo insta accept`). Never commit `.snap.new`
- **System deps**: `pkg-config` and `libasound2-dev` (only for the default `alsa` feature of `tidal-player-audio`). `cargo test -p tidal-player-audio --no-default-features` works without them
- **Release publish**: `cargo xtask publish` publishes the crates whose version crates.io lacks, in dependency order (the release workflow runs it); `--dry-run` prints the plan and publishes nothing
- **Dependencies**: every third-party crate is declared once in the root `[workspace.dependencies]`; crates use `dep = { workspace = true }`. Merges regenerate `Cargo.lock` with cargo, never by hand

## Development approach: tech-lead

The primary session works as a **tech-lead**, following the `tech-lead` skill at `.claude/skills/tech-lead/SKILL.md`, a symlink into the `.claude/vendor/agent-skills` submodule ([akunzai/agent-skills](https://github.com/akunzai/agent-skills/blob/main/skills/tech-lead/SKILL.md), MIT, see the submodule's `LICENSE`). Dependabot opens a PR when upstream moves; after cloning or pulling, run `git submodule update --init` (or set `git config submodule.recurse true`). Read it, and its `references/brief-elements.md`, before starting any non-trivial feature or fix:

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

A change is done when: the spec is `approved` (or `implemented`) and matches the code; every acceptance criterion it touches has a passing test; every "Build & tooling" check is clean; the feature has its entry under "Features" (an introduction, not a change log); `README.md` and the zh-TW copies are up to date (see "Conventions"); and the commit history shows the red commit before the green one.

## Conventions

- Commits follow Conventional Commits (`feat(ui): …`, `fix(player): …`, `test: …`, `docs(spec): …`)
- Every change is made in a new git worktree on its own branch, cut from `origin/master`. The main checkout (the repository root) stays on `master` and is never used to edit
- Branches are named `<kind>/<slug>`, the kind one of `feat` (a feature), `bugfix` (a bug fix), `docs` (documentation, specs, this file) or `chore` (CI, tooling, dependencies, anything else). Only `feat/*` and `bugfix/*` (and `master` and tags) run the CI build and tests (spec 0001 AC9), so a change to Rust code goes on one of those
- Every feature and bug fix has a GitHub issue. Starting one that has none, open it first (`gh issue create`): what the feature does, or the bug's expected vs. actual behaviour, and the spec it touches, so PRs, commits and specs can link to it
- Working on a GitHub issue: label it `whipping` when work starts (`gh issue edit N --add-label whipping`). The PR's description has `Closes #N` for each issue it resolves. When the PR is merged, swap the issue's label for `whipped` (`gh issue edit N --remove-label whipping --add-label whipped`)
- One spec → one PR where practical; a PR description lists the spec and the acceptance criteria it covers
- Versions: a release bumps only the crates whose code changed since the last release, by SemVer (a fix → patch, a feature → minor, a breaking change → major). A crate whose version leaves the shared one sets `version = "x.y.z"` in its own `Cargo.toml` instead of `version.workspace = true`; `[workspace.package] version` stays the version of the crates not bumped, and a bumped library crate's entry in `[workspace.dependencies]` gets its new version. `Cargo.lock` follows through `cargo update --workspace`. The release tag `vX.Y.Z` is the `tidal-player` binary's version
- Release notes: every tagged release gets a `## New in vX.Y.Z` section on its GitHub release, written fresh for that release. The `Release` workflow creates the release with the install steps (`.github/release-notes.md`) and GitHub's generated PR list; once it has, add the section above `## Install` (`gh release edit vX.Y.Z --notes-file …`, keeping the rest). One bullet per user-visible feature or fix since the previous tag: its name in bold, what it does for the user in a sentence or two, and a link to its wiki page. Write it for users, like the "Features" introductions, not a copy of commit or PR titles
- Every PR updates `README.md` so it matches the PR's result: what works, how to use it, links to the user docs. A PR that changes nothing user-visible still checks it and says so in its description
- User docs have a Traditional Chinese (Taiwan) translation: `README.zh-TW.md` and `docs/zh-TW/<page>.md`, one per `docs/<page>.md`. English is the source; a PR that changes `README.md` or a user doc updates its zh-TW copy in the same PR, and a new user doc gets its zh-TW copy at once. Each copy starts with a language switcher line (`English | [正體中文](…)`; the language is called 正體中文, never 繁體中文), keeps code, keys, commands, paths and config names untranslated, puts `<a id="english-slug"></a>` before each heading so English anchors keep working, and links to specs (English only) and examples by their real paths. Specs and this file are English only
- `docs/` (minus `docs/specs/`) is the source of the GitHub wiki (spec 0015): edit the docs there, never on the wiki, which every publish overwrites. A new user doc gets a row in `PAGES` (`xtask/src/wiki.rs`), or `cargo xtask wiki` fails. Where the program or README sends a user to the docs, link the wiki page, not `docs/`
- Keep this file for rules, stable facts and the "Features" introductions. Implementation detail belongs in specs and code comments — tidalt's CLAUDE.md turned into a per-file changelog that went stale

## Carried over from tidalt (facts to re-verify, not requirements)

These cost time in tidalt. Re-check each against the live API or hardware before a spec relies on it.

- Tidal auth uses the OAuth2 device flow; sessions need refresh-token handling and a recovery path when the refresh fails
- Daily Mixes: the v2 `openapi.tidal.com/v2/userRecommendations` resource was removed and 404s. tidalt used v1 `GET /v1/pages/my_collection_my_mixes` for the list and `GET /v1/mixes/{mixId}/items` for tracks; video mixes and non-`track` items must be filtered out
- Daemon-client mode (kept, see spec 0001 "Run modes"): one binary ran standalone, as a headless daemon (systemd user service, controllable by MPRIS clients such as `playerctl`), or as a TUI client of that daemon over D-Bus. The daemon held the audio device only while playing and released it on pause so other apps could use it
- Stream quality ladder: `HI_RES_LOSSLESS` → `LOSSLESS` → `HIGH` → `LOW`
- Bit-perfect output meant opening ALSA `hw:` directly, negotiating the PCM format per device (some DACs advertise a broken `S16_LE` endpoint, so 16-bit sources preferred `S32_LE` first), asking PipeWire to release the card via `org.freedesktop.ReserveDevice1`, and falling back to `plughw:` only when format negotiation is refused — never on a busy device
