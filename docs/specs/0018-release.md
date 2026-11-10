# 0018 — The release workflow

- **Status**: implemented (2026-10-24; Bug 4, AC10 and AC11 added 2026-11-11)
- **Owner**: tech-lead (primary session)
- **Issue**: [#50](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/50), [#73](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/73) (Bug 3)
- **Depends on**: 0001 (implemented: the workspace's crates and their dependency rules)
- **User docs**: none (maintainer tooling); `CLAUDE.md` "Build & tooling" names `cargo xtask publish`

## Context

Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml` (added in #26, #29, #31 without a spec): it checks the tag against `tidal-player`'s version, builds the binary, packages it with the source and their SHA-256s, waits for the tag's CI run, publishes the crates to crates.io and creates the GitHub release. Since #43 a release bumps only the crates whose code changed, so a release can leave a crate at a version crates.io already has. The publish step, `cargo publish --workspace`, cannot skip those, and `v1.2.0` failed there (Bug 1).

## Behaviour

Unchanged from today except the publish step:

1. The tag must equal `v` + `tidal-player`'s version, else the run fails before anything is built
2. The release binary is built; `tidal-player-vX.Y.Z-src.zip`, `tidal-player-vX.Y.Z-x86_64-unknown-linux-gnu.zip` and `SHA256SUMS` are made
3. The run waits for the CI run of the push to `master` on the tagged commit (`GITHUB_SHA`) and fails if it fails, or if there is none (a crates.io upload cannot be taken back). A release tag is pushed on a PR's merge commit, which `master`'s CI has already checked; a tag push runs no CI of its own (Bug 3, spec 0001 AC9), so a tag on a commit that is not on `master` is refused
4. **Publish** (`cargo xtask publish`): every workspace crate that may be published (not `publish = false`) whose version is **not yet on crates.io** is published, in dependency order (`tidal-player-core` before `tidal-player-api` and `tidal-player-audio`, those before `tidal-player`). A crate whose version is already there is skipped and named (`tidal-player-api 1.1.0: already on crates.io`). A version crates.io has yanked counts as already there (it refuses a re-upload). No new version: `nothing to publish`, and the run goes on
5. The GitHub release is created: `.github/release-notes.md`'s install steps, GitHub's generated notes, and the three files (then the maintainer adds its `## New in` section, `CLAUDE.md` "Conventions")

`cargo xtask publish --dry-run` prints the same plan and publishes nothing, so the plan can be checked locally before tagging.

**What is on crates.io** is read from the crates.io sparse index (`https://index.crates.io/<path>`, one JSON object per line with its `vers` and `yanked`; `<path>` is `1/<name>`, `2/<name>`, `3/<n>/<name>` or `<na>/<me>/<name>` by the name's length, lower-cased). A 404 means the crate was never published, so all its versions are new. The index is fetched with `curl` (on the runner and on any developer machine), behind a seam the tests fake.

**Publishing** a crate runs `cargo publish -p <name>`, which waits until crates.io has it before the next one starts. The first failure stops the run with that crate named; crates published before it stay published (a re-run then skips them, by step 4).

## Acceptance criteria

`xtask` (`xtask/src/publish.rs`, pure functions plus a command over two seams: the index fetch and the process runner):

- **AC1** — The plan (table): input the workspace's crates (name, version, publishable, workspace dependencies) in any order and each crate's versions on crates.io; output the crates to publish in dependency order and the ones skipped. Rows: every version new; today's case (`api` and `audio` at 1.1.0 already published, `core` and `tidal-player` at 1.2.0 new → `core`, `tidal-player`; `api` and `audio` skipped); none new → empty; a crate never published → planned; `publish = false` (`xtask`) → neither planned nor skipped; a yanked version → skipped; the input order shuffled → the same dependency order
- **AC2** — The index (table): the path for names of length 1, 2, 3, 4 and longer, upper case lower-cased; parsing index lines (a fixture in the index's real format, two versions, one yanked) into versions with their yanked flag; blank lines ignored; a line that is not JSON or has no `vers` → an error naming the crate and the line number
- **AC3** — The command (fake fetcher and runner, table): `publish` fetches each publishable crate once, then runs `cargo publish -p <name>` for each planned crate in order and prints each skip; a 404 → never published; a fetch failure other than 404 → an error and **no** `cargo publish` run; a failing `cargo publish` → stops there, the error names the crate, later crates are not run; `--dry-run` → prints the plan and runs nothing; an empty plan → prints `nothing to publish` and succeeds
- **AC4** — The workflow: `release.yml`'s publish step runs `cargo xtask publish` with `CARGO_REGISTRY_TOKEN`, and no step runs `cargo publish --workspace` (a test reads the file)
- **AC5** — Re-released `v1.2.0` (by hand, at acceptance; it needs the real crates.io and a tag): the run publishes `tidal-player-core` 1.2.0 and `tidal-player` 1.2.0, skips `api` and `audio` 1.1.0, and creates the GitHub release with its three files. No automated test: it would publish for real
- **AC6** — Pinned internal dependencies (Bug 2): every dependency of a workspace crate on another workspace crate that carries a version requires exactly that version (`=x.y.z`, a single `=` comparator), so a published crate needs the crates it was released with (a test reads the real workspace)
- **AC7** — The release's CI gate (Bug 3; a test reads `release.yml`): the step before the publish step that waits for CI lists `ci.yml` runs with `--event push`, `--branch master` and `--commit "${GITHUB_SHA}"`, never with the tag's name (`GITHUB_REF_NAME`) as the branch; with no run it fails (`exit 1`); otherwise it watches the run with `--exit-status`, so a failed or cancelled run fails the release
- **AC8** — CI runs once per commit (Bug 3; a test reads `ci.yml`): its triggers are exactly `push` to `branches: [master]` and `pull_request`, so a tag push and a push to any other branch run nothing; every job's `if` is `github.event_name == 'push' || startsWith(github.head_ref, 'feat/') || startsWith(github.head_ref, 'bugfix/') || startsWith(github.head_ref, 'refactor/')` (master's pushes, and pull requests from `feat/*`, `bugfix/*` and, since [0026](0026-realignment.md), `refactor/*`, which runs CI and is not a release)
- **AC9** — The first release after the fix (by hand, at acceptance, like AC5): on its merge commit Actions shows one CI run (the push to `master`) and none for the tag, and the release's wait step watches that run, then publishes. No automated test: it needs a real release
- **AC10** — Stale dependents (Bug 4; table): the index entry of each crate the plan skips (already on crates.io at its version) says which workspace crates that version needs; where it needs one exactly (`=x.y.z`, AC6) and the workspace's version of that crate is another, `publish` fails before any `cargo publish`, under `--dry-run` too, after the plan's lines, with one line per such requirement: `<dependent> <version> on crates.io needs <dependency> =<old>, but this release has <dependency> <new>: bump <dependent>`. It holds whether the dependency is being published in this run or was published by an earlier, failed one. Requirements that are not exact (the releases before AC6), dev-dependencies and crates outside the workspace are not compared; with no such requirement it runs as before
- **AC11** — The dry run in CI (Bug 4; a test reads `ci.yml`): the `check` job runs `cargo xtask publish --dry-run` after `cargo xtask specs`, so a release PR with a stale dependent fails before it is merged and tagged

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| crates.io unreachable, or the index answers 5xx | Error before anything is published; re-run the job |
| A version published between the check and the upload (two runs at once) | `cargo publish` fails on it and the run stops; `concurrency: release` already keeps runs one at a time |
| A new crate added to the workspace | Never on the index (404): published with the others, in dependency order |
| A crate published, a later one failed | The published one stays; a re-run skips it and publishes the rest |
| `curl` missing | Error naming it; nothing published |
| A tag on a commit that is not on `master` (no `master` CI run for it) | After the wait (5 minutes), error `no CI run on master for <sha>: tag a commit of master`; nothing published |
| `master`'s CI run on the tagged commit failed or was cancelled | The wait fails; nothing published. Fix on a new PR and tag its merge commit, or re-run the CI run (if it was flaky) and then the release job |
| `master`'s CI run still in progress when the tag is pushed | The release waits for it |

## Test plan

Red is a failing assertion against stub functions with stub bodies (no `todo!()`, no compile errors).

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `xtask/src/publish.rs` :: `ac1_plan` (table) | planned and skipped crates, in order | stub `plan` plans every publishable crate in input order |
| AC2 | `xtask/src/publish.rs` :: `ac2_index_path` (table), `ac2_parse_index` (table) | paths; versions and yanked flags; errors | stub path is the bare name; stub parser returns no versions |
| AC3 | `xtask/src/publish.rs` :: `ac3_publish_command` (table) | fetches, `cargo publish` calls and their order, output, errors | stub command runs `cargo publish -p` for every crate |
| AC4 | `xtask/tests/release_workflow.rs` :: `ac4_workflow_uses_xtask_publish` | the step's `run:` and no `--workspace` publish | the workflow still has `cargo publish --workspace` (the red commit leaves it) |
| AC5 | by hand at acceptance (see AC5) | the real release | — |
| AC6 | `xtask/tests/workspace.rs` :: `ac6_internal_dependencies_are_pinned` | every internal dependency's requirement is one `=` comparator | `[workspace.dependencies]` still has caret requirements (`^1.2.1`) |
| AC7 | `xtask/tests/release_workflow.rs` :: `ac7_release_waits_for_master_ci` | the wait step's `gh run list` flags, its `exit 1` with no run, `gh run watch --exit-status`, and that it comes before the publish step | the step still lists runs with `--branch "${GITHUB_REF_NAME}"` (the red commit leaves `release.yml` as it is) |
| AC8 | `xtask/tests/release_workflow.rs` :: `ac8_ci_runs_once_per_commit` | `ci.yml`'s `on:` block (comments and blank lines dropped) is exactly the three trigger lines; every job (`runs-on:`) has one `if:` equal to the expected one | `on:` still has a bare `push:` (the red commit leaves `ci.yml` as it is) |
| AC9 | by hand at acceptance (see AC9) | the first real release after the merge | — |
| AC10 | `xtask/src/publish.rs` :: `ac10_stale_dependents` (table) | the failure and its lines; nothing published; dry run too; no failure without one | stub finds no stale dependent, so the run succeeds |
| AC11 | `xtask/tests/release_workflow.rs` :: `ac11_ci_dry_runs_the_publish` | the step's `run:` and that it follows the specs step | `ci.yml` has no such step |

## Bugs

### Bug 1 — `cargo publish --workspace` fails when a crate keeps a published version (#50)

- **Expected**: the `v1.2.0` tag publishes `tidal-player` 1.2.0 and `tidal-player-core` 1.2.0 and creates the release
- **Actual**: run 40090212356 failed in "Publish to crates.io": `error: crate tidal-player-api@1.1.0 already exists on crates.io index`. Nothing was uploaded, and no release was created
- **Root cause**: `cargo publish --workspace` publishes every publishable crate and has no option to skip a version already on crates.io; #43's rule leaves unchanged crates at their published version
- **New criteria**: AC1 (today's case), AC4. The `v1.2.0` tag is moved to the merged fix and pushed again (AC5)

### Bug 2 — internal dependencies use caret requirements (#74)

- **Expected**: `cargo install tidal-player --version <an older version>` builds the crates that version was released with
- **Actual (risk)**: `[workspace.dependencies]` gives `tidal-player-core`, `-api` and `-audio` caret requirements (`version = "1.2.1"`). A fix or feature is a patch release and the major version stays `1` (`CLAUDE.md` "Versions"), so a later `1.x` of a library may change its API incompatibly. Without `--locked`, cargo resolves an older `tidal-player` against the newest `1.x` and the install fails to compile
- **Root cause**: the requirements follow cargo's default (`^`), which assumes semver compatibility the release rule does not promise
- **New criteria**: AC6. The `[workspace.dependencies]` entries of the internal crates use `version = "=x.y.z"`, and a bumped crate's entry gets `=<new version>`. Applies from the first release after the fix; published versions up to v1.2.1 keep `^` (`cargo install --locked` works around it)

### Bug 3 — every release commit (and every PR push) runs CI twice (#73)

- **Expected**: each commit is checked once, and nothing is published unless CI passed on the released commit
- **Actual**: `ci.yml` runs on every `push` and `pull_request`. Merging a PR pushes its merge commit to `master` (a CI run), then the release tag is pushed on the same commit (a second, identical run): `v1.2.1` on `d04867c` ran [master](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/runs/40168880426) and [the tag](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/runs/40169113802). A `feat/*` or `bugfix/*` branch with an open PR runs a `push` and a `pull_request` run for every push
- **Root cause**: the triggers are bare `push` and `pull_request`, and the release (step 3) waited for the tag's own CI run, so the tag's run could not be dropped
- **New criteria**: AC7 (the release waits for `master`'s run on `GITHUB_SHA`), AC8 (CI on pushes to `master` and pull requests only; spec 0001 AC9 changed with it), AC9 (the first release after the fix)

### Bug 4 — a bumped library does not force its dependents to bump (#99)

- **Expected**: a release whose crates cannot be published together fails on its PR
- **Actual**: `v1.5.0` bumped `tidal-player-core` to 1.5.0 and left `tidal-player-api` at 1.3.4 (its code had not changed). With AC6's exact pins the published `tidal-player-api 1.3.4` needs `tidal-player-core =1.4.4` and `tidal-player 1.5.0` needs `=1.5.0`, so `cargo publish -p tidal-player` could not resolve. `cargo xtask publish --dry-run` printed the plan and said nothing, CI passed, and the [Release run](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/workflows/release.yml) published `tidal-player-core 1.5.0` and failed on `tidal-player`: no `tidal-player 1.5.0`, no GitHub release
- **Root cause**: AC6 made "a crate depends on a bumped crate" a reason to bump it, and nothing checked that. The index says what each published version needs, and `publish` never read it
- **New criteria**: AC10 (`publish` compares what each skipped crate's published version needs with the workspace's versions), AC11 (CI runs the dry run). `CLAUDE.md` "Versions" says so too
- **Recovery**: `tidal-player-api` 1.3.5 pins `tidal-player-core =1.5.0`; the tag `v1.5.0`, which no release was built from, moves to the fix's merge commit (the maintainer's decision), and the workflow publishes `tidal-player-api 1.3.5` and `tidal-player 1.5.0`

## Implementation notes

- Output lines: `<name> <version>: already on crates.io` for a skip, `publishing <name> <version>` before each `cargo publish`, `would publish <name> <version>` under `--dry-run`; skips print first, then `nothing to publish` when nothing is planned
- All index fetches happen before the first `cargo publish`, so a crates.io outage uploads nothing; the skipped list is in dependency order too
- The fetcher runs `curl -sS -w '\n%{http_code}'` and reads the status from the last line; 200 is an entry, 404 never published, anything else an error
- The dependency order uses normal and build dependencies between members (`cargo metadata --no-deps`); ties go by name

## Out of scope

- Publishing anything other than crates.io (AUR, binaries for other targets)
- Writing the `## New in` section automatically
- Checking that a changed crate's version was bumped (the maintainer's rule, `CLAUDE.md` "Conventions")
