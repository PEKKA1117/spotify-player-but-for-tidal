# Brief elements — detail

## Durability gate (step 1)

Ask before slicing whether the target surface is still in active use or about
to be replaced. Git history only suggests an answer — a file untouched since
2022 is consistent with both "abandoned" and "stable and correct" — it does
not decide, and the real answer often lives only in the user's head. Skipping
this once meant a 19-test batch got dispatched against a page whose capability
had already been split into a separate product with its own backend.

## Parallel cap (step 2)

Ask for both answers in one turn: whether to parallelize this run at all,
and the concurrency cap. The cap defaults to 3; a cap below 2 counts as
serial, and a non-positive number gets re-asked. A decline marks every
parallel-marked slice serial and skips the conflict scan below.

A lower subscription tier or a tight token budget both argue for declining
parallelism; asking up front costs one turn and avoids opening subagents
that then have to be aborted.

## Parallel-conflict signals (step 2)

Scan for explicit slot allocation (docker-compose services, per-branch
ports/`.env`), a single shared mutable resource two slices would both hit
(one dev server port, one database, one lockfile), and any parallel-safety
note already in AGENTS.md/CLAUDE.md. A signal you cannot rule out counts as
a conflict, not a clear reading. A conflict downgrades every affected slice
to serial and you tell the user why; slices clear of the conflict keep the
confirmed cap.

## Branch basing (step 5)

When a slice's allowed files overlap an unmerged slice's, base its branch on
that slice's branch instead of the default branch. Basing on default
guarantees a conflict in exactly the files the brief allows the new slice to
touch — the overlap step 1 already recorded is not hypothetical.

## Evidence capture (steps 4 and 7)

A brief listing only a screenshot CLI once captured a localized app in the
automation's default English, in a repo whose rules preferred a walkthrough
recorder. The scenario carries what that brief lacked.

At step 4, write the scenario into the slice's acceptance:

- the entrypoint and port, the fixture data, and each step with the
  observation you expect from it
- the project's capture rules, wherever it keeps them — a verification or
  contributing doc, a request template, `AGENTS.md` — or a note that none
  exist
- the capture skills from the visible list whose descriptions match the
  platform (a web walkthrough recorder, a terminal recorder), in preference
  to the tool the rules call a fallback
- where the UI is localized, the UI locale the capture applies (browser
  automation starts in `en-US`, whatever the developer's own browser says),
  plus anything known to override it, such as a locale cookie

At step 7, after the implementer returns, run the scenario in a fresh
context: an `evidence-collector` worker where one exists, which keeps
snapshots and frames out of your context and does not grade its own work.
Keep a screenshot or two, and any capture you are debugging from, in
primary; on Cursor CLI run it in primary, since its `tools:` boundary is
not enforced there. The worker returns a verdict per step; the acceptance
verdict is yours. On a mismatch, read the capture paths it returned rather
than dispatching it again. Mask the regions it flags before anything is
attached.

## Wave dispatch (step 6)

Launch one wave's dispatches together, then wait for that whole wave to
clear step 7 (Accept) before starting the next. A parked slice counts as
cleared; a resumed implementer still holds its wave slot. Announce the plan
— slice count, cap, wave count — before the first dispatch.

## Facts vs assumptions (step 6)

Name which claims in the brief you verified from source and which are your
own guesses for the implementer to check. Mixing them unmarked teaches the
implementer to distrust the whole brief, or — worse — to trust the wrong
part. A wrong assumption about infrastructure (assuming a data store needed
rebuilding when a script already re-seeded it on every run) cost one
implementer a wrong turn before it caught the mistake on its own.

## Defect-report shape (steps 6–7)

For a test-writing or audit slice, the defect report has a fixed shape: what
you did, what happened, what should have happened, the file:line responsible,
and whether the test asserts the current wrong behaviour or skips it. Report,
do not fix, unless the brief's scope says otherwise. Three real batches found
4, 8, and 4 defects respectively — this is the normal outcome of that kind of
slice, not an edge case.

A returned defect report never files a ticket by itself. Verify it — the
implementer's report may itself be a suspicion, not a confirmed defect, and
verifying can also reveal it is worse than flagged — then file only once the
user agrees. Filing on the report alone skips both checks.

## Slice-only resources (step 8)

A worktree and its branch are not the only things a slice can leave behind —
a brief that had the implementer spin up a container, seed a database, or
allocate any other resource scoped to that slice alone owes the same cleanup.
Shared infrastructure (a project-wide dev environment, a CI runner) is not
in scope here; only what the brief created for this slice specifically.
