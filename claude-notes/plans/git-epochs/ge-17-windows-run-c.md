---
title: 'GE-17: Windows run C: the scripted scenario and the Playwright subset'
date: 2026-11-18
description: Runs the scripted end-to-end scenario and the permanent Playwright VS Code subset on a Windows machine, repeats every earlier Windows item on the final code, and records what Windows does and does not prove for the epic.
status: approved
depends-on: [ge-14, ge-15, ge-16]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-14](ge-14-cross-client-e2e.md), [GE-15](ge-15-windows-run-a.md), and
[GE-16](ge-16-windows-run-b.md) · **Unblocks:** [GE-18](ge-18-docs-and-closeout.md) ·
**Branch:** `ge/17-windows-run-c` off `feature/git-epochs` for the notes, merged by rebase and fast-forward. The tooling
stays on the temporary `windows/git-epochs` branch, which is extended here and never merged. The only push in this plan
is that branch, after Gordon says go.

## Goal

Close the Windows question for the epic. Run the whole new feature end to end on Windows with the real `q2.exe`: a
`--git track` hub on a temp repo, a headless clone peer, and real `git switch` operations. Run the permanent Playwright
VS Code subset on Windows. Then run every item from GE-15 and GE-16 again on the final code, because fix plans may have
landed since those runs. The deliverable is a final statement of what Windows does and does not prove.

## Decisions implemented

- **G31:** the third Windows run, from the tooling branch, with the `windows/` remote prefix. Only new tests run.
- **G30:** the headless peer and the Playwright subset that GE-14 marked permanent (local hub, no network).
- **G33:** the hub is `q2.exe preview --git track`; the `/api/preview/git-status` route is observed during a real
  operation.
- **G3:** the gate markers are observed during a real `git switch` of a large tree.
- **G9, G13:** arrival and unlist and relist are asserted on a real Windows checkout, including a binary file.
- **G5:** shutdown while a marker is set skips the flush.
- **G32:** the timeouts used by the scenario are the epic\'s numbers.

## Context

- **What earlier plans leave.** GE-15 and GE-16 left `scripts/git-epochs-windows/run.ps1` with items `a*` (Rust and hub)
  and `b1` to `b7` (TypeScript and extension), the failure extractor, the instructions page, and the large-tree
  generator. Read their notes first, including any fix plans inserted after them.
- **What GE-14 left.** The cross-client e2e scenarios and the list of tests it marked permanent, with the command that
  runs them. Read its notes for the exact tag or file list and the Playwright config path; this plan uses them as
  written.
- **The model for the scenario** is GE-13\'s hub e2e (git helpers, the same steps) run through the real binary instead
  of a harness on macOS. The Windows version is a script that drives `q2.exe`, a headless peer, and `git.exe`.
- **Process lessons from RFU-6.** Run each command through `cmd.exe` and wait on that one process (leaked processes hold
  a pipe open); merge the integration tip into the tooling branch first (never rebase a pushed branch) and record the
  base commit; clone with
  `core.autocrlf=true` and report anything else.

## Test specifications

| Item | How it is tested | A pass proves | It does not prove |
|---|---|---|---|
| `c1` (W9) | The scripted scenario below, against the real `q2.exe` and `git.exe` on a generated temp repo in `%TEMP%`; the script exits non-zero if any assertion fails and writes a JSON transcript of each step and the observed values | The feature works end to end on Windows: the gate, the classifier, the registry, the flips, the unlists, a binary file, and shutdown with a marker set | jj on Windows; other Git builds; long soak |
| `c2` (W8) | The Playwright subset GE-14 marked permanent, with the pinned VS Code build that the harness downloads (`downloadAndUnzipVSCode`, the same version as on Linux),
launched the same way the Linux harness launches it | The extension\'s VS Code surfaces (commands, notifications, status bar, buffers) work with `Code.exe` | VS Code Insiders or other versions; the unmarked exploratory tests |
| `c3` | Every item from GE-15 (`a*`) and GE-16 (`b1` to `b7`), run in one invocation without `-Only`, on the final code | Nothing regressed on Windows after the fix plans; the whole set is green together | Anything the items did not already cover |

**The scenario for `c1`** (each step asserts; the transcript records what it saw):

1. Generate the deterministic repo (the generator from GE-15, with text files, one binary, and nested directories) on
   `main`; create a `feature` branch that changes one text file, removes one file, adds one file, and changes the binary.
2. Start `q2.exe preview --git track --allow-edit --ui editor --no-browser --port N` (N free) in the repo.
3. Attach a headless clone peer from a second clone by the Join logic (`/health` gives the index id), and type an edit
   into one file.
4. Commit on `main` in the hub\'s repo while the peer edits: no flip, the peer\'s edit intact.
5. `git switch feature` on the hub\'s repo, polling `.git/index.lock` and `/api/preview/git-status`: the route reports
   paused at least once and not paused after; the changed file flips, the removed file is unlisted, the added file is
   listed, the binary\'s doc changes; the peer, still on `main`, keeps its docs and its disk is untouched.
6. `git switch main`: arrival re-attaches the registered docs (the pointer returns to the earlier docIds, no fork), and
   disk equals the doc.
7. Save a file and commit inside the debounce window: handled as a commit (no flip).
8. `git reset --hard` after a dirty edit: the pointer moves to a new doc equal to the HEAD content, and the dirty text
   stays in the retired doc.
9. In a second run, send Ctrl-C to the hub during a `git switch` of the large tree: the shutdown flush is skipped
   while the marker exists, and no doc contains conflict markers or half-written text (detail chosen by the plan
   author; confirm).

## Checklist

Items marked (agent) run on macOS; items marked (Gordon) need the Windows machine.

- [ ] (agent) Confirm GE-14, GE-15, and GE-16 are complete, and read the notes of all three, including every fix plan
  inserted after the earlier Windows runs.
- [ ] (agent) Merge the final `feature/git-epochs` tip into `windows/git-epochs` (never rebase the pushed branch) and
  record the base commit hash.
- [ ] (agent) Write the scenario script for `c1` on the tooling branch (a Node script that drives `q2.exe`, the headless
  peer, and `git.exe`, using GE-13\'s steps as the template). Dry-run its logic against the macOS `q2` binary to check
  the assertions before handoff.
- [ ] (agent) Add items `c1`, `c2`, and `c3` to `run.ps1`; `c2` uses the command and config GE-14 recorded; `c3` runs
  every `a*` and `b*` item. Each fills `proves` and `does_not_prove`.
- [ ] (agent) Update the instructions page on the tooling branch: the pinned VS Code build the harness downloads (record the version), Node at the
  `.nvmrc` major, `core.autocrlf=true`, `core.longpaths=true`, optional `jj`, and the table above.
- [ ] (agent) Gate the tooling: parse-only syntax check of the script with `pwsh` if installed; the extractor\'s help;
  `q2 render --strict` of the instructions page; the Playwright subset passes on macOS or Linux first.
- [ ] (agent) Ask Gordon to say go, then push the tooling branch, unsilenced:
  `git push origin windows/git-epochs:windows/git-epochs`, and check `git ls-remote origin windows/git-epochs` against
  `git rev-parse HEAD`.
- [ ] (Gordon) On the Windows machine, fetch and switch to the branch and run
  `powershell -ExecutionPolicy Bypass -File scripts\git-epochs-windows\run.ps1 -Publish`. Run the extractor once if
  anything failed and push the details.
- [ ] (agent) Fetch, parse `results/*.json`, and record in the notes: run id, host, Windows version, git, node, VS Code
  version, `core.autocrlf`, the base commit, the table of item results, the `c1` transcript, and every failure with
  its output.
- [ ] (agent) For each failure decide whether it is a product defect, a test defect, or the machine, and propose a fix
  plan. A fix plan inserted here means this plan stays open until its follow-up run passes; do not change product
  code in this plan.
- [ ] (agent) Write the final statement in the notes (below) and copy the summary into `ge/17-windows-run-c`; rebase,
  fast-forward, and leave the tooling branch unmerged. Do not delete the tooling branch: GE-18 does that, with
  Gordon\'s agreement, after the notes are recorded.

## The final statement

Write this in the notes, filled from the results. A pass proves compilation and execution of the listed tests with a
real `git.exe`, `q2.exe`, and Node on one Windows machine, and the end-to-end scenario above. It does not prove: jj on
Windows unless installed; other Git builds or Windows versions; antivirus, OneDrive, or Dev Drive effects; a long
soak; VS Code beyond the permanent subset; the public sync server; or the iroh tunnel across real networks.

## Gates

- The tooling gates in the checklist.
- If any test was added or changed on `ge/17-windows-run-c`: its own suite on macOS, `cargo xtask lint`, and
  `q2 render --strict` of the notes file.

## End-to-end verification

`c1` is the end-to-end verification on Windows, run through the real binary. Record the exact invocation, the
transcript excerpt for steps 4 to 6, and an explicit note that the output was inspected. If any step could not run,
say so instead of claiming success.

## Exit criteria

- Items `c1`, `c2`, and `c3` each have a result in the notes, with failures and their output.
- The final statement is written.
- Every failure is classified, with a proposed fix plan where needed.
- The tooling branch is unmerged and not yet deleted.

## Out of scope

- Fixing defects (fix plans follow); deleting the tooling branch (GE-18).
- Failures in older tests that predate the epic.
- A soak test or a performance benchmark.

## Notes from the run

Not started.
