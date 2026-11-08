---
title: 'OPR-4b: the Julia runner reports expression starts, anchors and output kinds'
date: 2026-11-08
description: 'Conditional on the OPR-4a spike and Gordon''s go. Implements, in a local branch of the QuartoNotebookRunner fork, the runner-side change the spike showed is feasible. The work happens in another repository; this plan lands only a note in q2. Gordon pushes the runner branch and records the pinned revision, which unblocks OPR-4c.'
status: conditional
depends-on: [opr-4a]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/4b-julia-runner-facts` · **Lane B** (`workspace-4`) · **Blocks:** OPR-4c.
**Needs:** Julia 1.12, a git worktree of the runner repo (`gordonwoodhull/QuartoNotebookRunner.jl`; a checkout is at
`/Users/gordon/src/q2/external-sources/QuartoNotebookRunner.jl`), and Gordon\'s go.

## Start condition

Do **not** start unless this plan\'s `status` is `ready` on the integration branch. Gordon sets it after reading the
OPR-4a note and writes the scope he chose (which of the three facts, in what shape) into the section "Scope chosen
by Gordon" below. If that section is empty, stop and report.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/4b-julia-runner-facts feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# the runner work happens in a separate worktree of the runner repo (below)
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/4b-julia-runner-facts
```

## Scope chosen by Gordon

(Empty until Gordon fills it in after OPR-4a.)

## Goal

Implement the runner change from the OPR-4a note, behind an option that is **off by default**, so nothing changes
for any existing caller: when the engine asks, the runner returns per unit the expression starts, the anchors
and (if in scope) the output kinds, in the optional wire fields the OPR-4a note proposed.

## Checklist (every box is `[unattended]` unless marked)

- [ ] Create a worktree of the runner repo on a new branch `q2/opr-expression-facts` from the revision pinned today
      in the engine (`resources/extension-subtrees/julia-engine/_extensions/julia-engine/Project.toml`; (relied)
      `5e042849`). Never modify the runner checkout\'s own branch. Do not push.
- [ ] **Tests first** in the runner\'s `test/testsets/` (see its README): the option off gives byte-identical results to
      today on the existing testsets; the option on gives the facts for the OPR-4a demonstration cell
      (`println`, a value, a plot display, an `@warn`), with line numbers.
- [ ] Implement per the OPR-4a note; run the runner\'s full test suite and record the result.
- [ ] Commit in the runner worktree with clear messages. Write the list of commits (hashes and subjects) into
      "Notes from the run", and put a `git format-patch` of them under the note\'s directory in this repository if it
      is small (under 200 lines); otherwise name the runner branch only.
- [ ] Land in q2 **only** a note `claude-notes/research/2026-11-xx-opr-julia-runner-facts.md` (what changed, how to
      reproduce, the test results) plus the plan\'s own status change. No q2 product code changes in this plan.
- [ ] Render-check the note.

## Exit criteria

- The runner branch exists locally with passing runner tests; the note records its commits and how to run the new
  behavior.
- **`[Gordon]` (after landing):** push the runner branch to the fork, then write the pushed revision into OPR-4c\'s
  header and set OPR-4c to `ready`. OPR-4c cannot start without that revision.

## Notes from the run

(Fill in at landing.)
