# 0018 — The release workflow

- **Status**: implemented (2026-10-24)
- **Owner**: tech-lead (primary session)
- **Issue**: [#50](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/50)
- **Depends on**: 0001 (implemented: the workspace's crates and their dependency rules)
- **User docs**: none (maintainer tooling); `CLAUDE.md` "Build & tooling" names `cargo xtask publish`

## Context

Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml` (added in #26, #29, #31 without a spec): it checks the tag against `tidal-player`'s version, builds the binary, packages it with the source and their SHA-256s, waits for the tag's CI run, publishes the crates to crates.io and creates the GitHub release. Since #43 a release bumps only the crates whose code changed, so a release can leave a crate at a version crates.io already has. The publish step, `cargo publish --workspace`, cannot skip those, and `v1.2.0` failed there (Bug 1).

## Behaviour

Unchanged from today except the publish step:

1. The tag must equal `v` + `tidal-player`'s version, else the run fails before anything is built
2. The release binary is built; `tidal-player-vX.Y.Z-src.zip`, `tidal-player-vX.Y.Z-x86_64-unknown-linux-gnu.zip` and `SHA256SUMS` are made
3. The run waits for the tag's CI run and fails if it fails (a crates.io upload cannot be taken back)
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

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| crates.io unreachable, or the index answers 5xx | Error before anything is published; re-run the job |
| A version published between the check and the upload (two runs at once) | `cargo publish` fails on it and the run stops; `concurrency: release` already keeps runs one at a time |
| A new crate added to the workspace | Never on the index (404): published with the others, in dependency order |
| A crate published, a later one failed | The published one stays; a re-run skips it and publishes the rest |
| `curl` missing | Error naming it; nothing published |

## Test plan

Red is a failing assertion against stub functions with stub bodies (no `todo!()`, no compile errors).

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `xtask/src/publish.rs` :: `ac1_plan` (table) | planned and skipped crates, in order | stub `plan` plans every publishable crate in input order |
| AC2 | `xtask/src/publish.rs` :: `ac2_index_path` (table), `ac2_parse_index` (table) | paths; versions and yanked flags; errors | stub path is the bare name; stub parser returns no versions |
| AC3 | `xtask/src/publish.rs` :: `ac3_publish_command` (table) | fetches, `cargo publish` calls and their order, output, errors | stub command runs `cargo publish -p` for every crate |
| AC4 | `xtask/tests/release_workflow.rs` :: `ac4_workflow_uses_xtask_publish` | the step's `run:` and no `--workspace` publish | the workflow still has `cargo publish --workspace` (the red commit leaves it) |
| AC5 | by hand at acceptance (see AC5) | the real release | — |

## Bugs

### Bug 1 — `cargo publish --workspace` fails when a crate keeps a published version (#50)

- **Expected**: the `v1.2.0` tag publishes `tidal-player` 1.2.0 and `tidal-player-core` 1.2.0 and creates the release
- **Actual**: run 40090212356 failed in "Publish to crates.io": `error: crate tidal-player-api@1.1.0 already exists on crates.io index`. Nothing was uploaded, and no release was created
- **Root cause**: `cargo publish --workspace` publishes every publishable crate and has no option to skip a version already on crates.io; #43's rule leaves unchanged crates at their published version
- **New criteria**: AC1 (today's case), AC4. The `v1.2.0` tag is moved to the merged fix and pushed again (AC5)

## Implementation notes

- Output lines: `<name> <version>: already on crates.io` for a skip, `publishing <name> <version>` before each `cargo publish`, `would publish <name> <version>` under `--dry-run`; skips print first, then `nothing to publish` when nothing is planned
- All index fetches happen before the first `cargo publish`, so a crates.io outage uploads nothing; the skipped list is in dependency order too
- The fetcher runs `curl -sS -w '\n%{http_code}'` and reads the status from the last line; 200 is an entry, 404 never published, anything else an error
- The dependency order uses normal and build dependencies between members (`cargo metadata --no-deps`); ties go by name

## Out of scope

- Publishing anything other than crates.io (AUR, binaries for other targets)
- Writing the `## New in` section automatically
- Checking that a changed crate's version was bumped (the maintainer's rule, `CLAUDE.md` "Conventions")
