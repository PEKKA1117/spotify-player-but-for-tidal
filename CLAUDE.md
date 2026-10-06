# CLAUDE.md

A terminal Tidal player modelled on [spotify-player](https://github.com/aome510/spotify-player). This is a from-scratch rebuild of [tidalt](https://github.com/PEKKA1117/tidalt), which grew too buggy to keep patching. Nothing is carried over as code; behaviour worth keeping is re-specified and re-tested here first.

## Status

Greenfield, in **Rust**. The toolchain and crate layout are set by `docs/specs/0001-architecture.md` (draft, awaiting approval); the "Build & tooling" section below is filled in from it once approved. Until then, don't add source code.

## Build & tooling

_To be filled in by spec 0001._ When it is, list exactly these, as commands an agent can run unattended:

- **Format**: the command, run after editing any source file
- **Lint**: the command, run before finishing a task; fix everything it reports
- **Build**: the command
- **Test**: the full suite, and how to run a single package/module
- **System deps**: native libraries, CGO/FFI requirements, anything CI must install

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
2. **Red (TDD)** — write a failing test per acceptance criterion (for a bug: a test that reproduces it). Run it and confirm it fails **for the expected reason**, not on a compile error or a missing fixture. Commit the red state on its own (`test: …`) so acceptance can check it out
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
- **Out of scope**
- **Bugs** (added over time): see step 1

## Definition of done

A change is done when: the spec is `approved` (or `implemented`) and matches the code; every acceptance criterion it touches has a passing test; every "Build & tooling" check is clean; and the commit history shows the red commit before the green one.

## Conventions

- Commits follow Conventional Commits (`feat(ui): …`, `fix(player): …`, `test: …`, `docs(spec): …`)
- One spec → one PR where practical; a PR description lists the spec and the acceptance criteria it covers
- Keep this file for rules and stable facts. Implementation detail belongs in specs and code comments — tidalt's CLAUDE.md turned into a per-file changelog that went stale

## Carried over from tidalt (facts to re-verify, not requirements)

These cost time in tidalt. Re-check each against the live API or hardware before a spec relies on it.

- Tidal auth uses the OAuth2 device flow; sessions need refresh-token handling and a recovery path when the refresh fails
- Daily Mixes: the v2 `openapi.tidal.com/v2/userRecommendations` resource was removed and 404s. tidalt used v1 `GET /v1/pages/my_collection_my_mixes` for the list and `GET /v1/mixes/{mixId}/items` for tracks; video mixes and non-`track` items must be filtered out
- Stream quality ladder: `HI_RES_LOSSLESS` → `LOSSLESS` → `HIGH` → `LOW`
- Bit-perfect output meant opening ALSA `hw:` directly, negotiating the PCM format per device (some DACs advertise a broken `S16_LE` endpoint, so 16-bit sources preferred `S32_LE` first), asking PipeWire to release the card via `org.freedesktop.ReserveDevice1`, and falling back to `plughw:` only when format negotiation is refused — never on a busy device
