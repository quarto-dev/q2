---
title: 'GE-17: Windows run C: the scripted scenario and the Playwright subset'
date: 2026-11-18
description: Runs the scripted end-to-end scenario and the permanent Playwright VS Code subset on a Windows machine, repeats every earlier Windows item on the final code, and records what Windows does and does not prove for the epic.
status: approved
depends-on: [ge-14, ge-15, ge-16, ge-16a]
lane: A
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-14](ge-14-cross-client-e2e.md), [GE-15](ge-15-windows-run-a.md),
[GE-16](ge-16-windows-run-b.md), and [GE-16a](ge-16a-windows-fixes.md) · **Unblocks:** [GE-18](ge-18-docs-and-closeout.md) ·
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
| `c2` (W8) | The Playwright subset GE-14 marked permanent, plus GE-9\'s timing test (`test:e2e:timing`, not in the permanent subset), with the pinned VS Code build that the harness downloads (`downloadAndUnzipVSCode`, the same version as on Linux),
launched the same way the Linux harness launches it. The timing test\'s measured gap (from `index.lock` release to the last buffer reload) is recorded in the results; a gap above the 1 s window is a finding for the experimental fallback in G20, not a failed run | The extension\'s VS Code surfaces (commands, notifications, status bar, buffers) work with `Code.exe`; editor-only\'s quiet window against a real Windows reload | VS Code Insiders or other versions; the unmarked exploratory tests |
| `c3` | Every item from GE-15 (`a*`) and GE-16 (`b1` to `b7`), run in one invocation without `-Only`, on the final code | Nothing regressed on Windows after the fix plans; the whole set is green together | Anything the items did not already cover |

**The scenario for `c1`** (each step asserts; the transcript records what it saw):

1. Generate the deterministic repo (the generator from GE-15, run in its large-tree mode of about 4,000 files, with a
   few text files, one binary, and nested directories, so that a `git switch` holds the lock long enough to observe; add
   a `--with-binary` mode to the generator in GE-15\'s tests if it lacks one) on `main`; create a `feature` branch that
   changes one text file, removes one file, adds one file, and changes the binary.
2. Start `q2.exe preview --git track --allow-edit --ui editor --no-browser --port N` (N free) in the repo.
3. Attach a headless clone peer from a second clone by the Join logic (`/health` gives the index id), and type an edit
   into one file.
4. Commit on `main` in the hub\'s repo while the peer edits: no flip, the peer\'s edit intact.
5. `git switch feature` on the hub\'s repo, polling `.git/index.lock` and `/api/preview/git-status` every 20 ms (the
   large tree of step 1 keeps the lock for long enough): the route reports paused at least once and not paused after; the changed file flips, the removed file is unlisted, the added file is
   listed, the binary\'s doc changes; the peer, still on `main`, keeps its docs and its disk is untouched.
6. `git switch main`: arrival re-attaches the registered docs (the pointer returns to the earlier docIds, no fork), and
   disk equals the doc.
7. Save a file and commit inside the debounce window: handled as a commit (no flip).
8. `git reset --hard` after a dirty edit: the pointer moves to a new doc equal to the HEAD content, and the dirty text
   stays in the retired doc.
9. In a second run, stop the hub during a `git switch` of the large tree: the shutdown flush is skipped while the
   marker exists, and no doc contains conflict markers or half-written text (detail chosen by the plan author;
   confirm). The hub\'s shutdown handler is `tokio::signal::ctrl_c()` (`crates/quarto/src/commands/preview.rs:698`),
   which on Windows listens for `CTRL_C_EVENT`, and a Node parent cannot send that event to one child process. The
   script therefore starts the hub in its own console through a small PowerShell helper and sends the event with
   `GenerateConsoleCtrlEvent` (P/Invoke); if that cannot reach the hub, step 9 is recorded as not run on Windows, and
   the skipped-flush behavior is the hub-level test of GE-3 (which stands in for the marker with a file).

## Checklist

Items marked (agent) run on macOS; items marked (Gordon) need the Windows machine.

- [ ] (agent) Confirm GE-14, GE-15, GE-16, and GE-16a are complete, and read the notes of all four. GE-16a carries three
  Windows steps to this plan: the baseline run of `main` at its merge base (`run.ps1 -Only a6`, GE-15 F3; each
  pre-existing failure becomes a braid strand), one run on a machine with Developer Mode on so that the two skipped
  symlink tests run (GE-16a `B2`), and the `B4` evidence: run the `b4` item with `QUARTO_HUB_DEBUG=1` in the environment
  (GE-16a added it; it logs each watcher event\'s path form, its relative key, and the clone gate state to the log and
  to stderr), and if `clone-modes.test.ts` "each decision writes its event to events.jsonl" still fails, read those
  `debug: disk` lines first. A follow-up fix plan for it, if needed, is inserted at the end of this plan.
- [ ] (agent) In the tooling worktree `/Users/gordon/src/q2/.worktrees/workspace-8/.worktrees/windows-git-epochs` (GE-15 created it inside lane B\'s worktree; the branch
  is checked out there, so do all tooling work in it), merge the final `feature/git-epochs` tip into
  `windows/git-epochs` (never rebase the pushed branch) and
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
