---
name: tech-lead
description: >-
  Tech-lead: split implementation into slices, brief a subagent, isolate
  independent slices in git worktrees, and accept the result in the
  primary session. Use when delegating implementation to subagents
  or running parallel worktrees.
license: MIT
metadata:
  capabilities: shell, network, git-history, forge-writes
---

# Tech Lead

You are the **tech-lead**. Implementers write code. You write the brief, isolate work, and accept.

Stay in primary for a few-line or single-file mechanical edit and for architecture decisions; delegate anything larger. Bounded investigation (search, facts) uses the platform's cheap named workers when those roles exist; tests, builds, and lint run in primary, which reads their output.

## 1. Slice

Check scope/durability with the user unless settled.

Split the work into units that can finish without each other's uncommitted files. For every pair that shares a path (in-review included), record whether the overlap is semantic (same meaning) or mechanical (lockfile regeneration).

Slice no finer than parallelism pays for. A serial slice that lands in the same review as the one it waits on merges into it, and a shared interface stays in one slice with the consumers that shape it; split apart, the brief would fix the interface before its callers could inform it. Ask how many PRs/MRs the user wants to review: that groups slices, it does not split them.

**Done** when each slice has a name, the files it may touch, a parallel/serial mark and durability answer, and earns its own implementer by running in parallel or needing a different role or model.

## 2. Confirm parallelism

Fewer than two parallel-marked slices: skip to step 3. Otherwise ask the user whether to parallelize and at what cap, then have a read-only worker scan for conflict signals first. Named: **Parallel cap**, **Parallel-conflict signals** — `references/brief-elements.md`.

**Done** when every parallel-marked slice is confirmed under a cap or serial with a reason.

## 3. Live capability

From this turn's system prompt and tool schemas, pick for each slice:

- the cheapest model and effort the harness exposes that can finish that slice
- an implementer role from the live list (general-purpose or equivalent)

Omitting the model parameter inherits the session model; do that only for high-judgment slices. Mechanical slices get an explicit cheaper pick.

**Done** when every slice names a live role and either an explicit model/effort or an explicit inherit.

## 4. Implementation skills

From skills this session can already see (harness skill list, or `skills ls --json` where present), pick 0–N whose descriptions match the slice and that teach how to implement (tests, toolchain, domain procedure). Review and audit skills wait for step 7.

Put absolute `SKILL.md` paths in the brief. That list is the full set the implementer reads.

A slice whose acceptance includes screenshots or a recording gets that evidence at step 7, from a scenario you write now into its acceptance. Named: **Evidence capture** — `references/brief-elements.md`.

**Done** when the brief lists those paths, or lists none, and a slice with visual evidence carries an **Evidence capture** scenario.

## 5. Isolate

Prefer harness-native worktree or cwd isolation. Lacking that, `git worktree add` beside the repo or in the harness default directory, named after the slice.

You create, collect, and remove worktrees. Each implementer receives one existing path. An overlapping slice branches off the other's, not default.

**Done** when each in-flight slice has a worktree path and a branch.

## 6. Brief and dispatch

One implementer per slice. Confirmed-parallel slices dispatch in waves of at most step 2's cap; the rest one at a time.

The brief is the slice: scope, allowed files, constraints, acceptance, skill paths from step 4, worktree and branch, and when to stop and return. The implementer may commit in its worktree.

Named: **Wave dispatch**, **Facts vs assumptions**, **Defect-report shape** (test/audit) — `references/brief-elements.md`.

**Done** when each dispatched slice has a live handle, no wave exceeded the cap, and its brief split facts/assumptions and (test/audit) held defect-report shape.

## 7. Accept

On return, record every item:

- brief constraints held
- claimed verification actually ran
- visual evidence: the **Evidence capture** scenario ran in a fresh context, and you judged its captures
- ship, ask the user, or resume the same implementer with findings
- a defect report: verify; ticket only with user agreement

When the slice is high-risk, you invoke an installed review skill (code review, over-engineering review, security) as coordinator.

Read an existing review artifact's findings and summary. Open the diff only if that artifact is empty or failed, the slice is high-risk, or a finding needs your ruling. Axes that review already completed stay closed.

**Done** when every slice is accepted or parked with a recorded reason.

## 8. Integrate

You push, open the MR/PR, merge, and remove the worktree, branch, and slice-only resources; rebase after merge. Confirm with the user first when the project requires it.

**Done** when accepted commits are on the intended branch or MR, or you recorded why not.
