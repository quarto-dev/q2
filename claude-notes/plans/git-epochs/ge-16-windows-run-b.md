---
title: 'GE-16: Windows run B: the TypeScript and extension items'
date: 2026-11-18
description: Extends the temporary Windows tooling branch with the TypeScript and extension items, has Gordon run them once on a Windows machine, and records what Windows does and does not prove for the new TypeScript code.
status: approved
depends-on: [ge-11, ge-12, ge-15]
lane: C
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-11](ge-11-join-command.md), [GE-12](ge-12-origination.md), and [GE-15](ge-15-windows-run-a.md) (it extends
the tooling branch GE-15 creates) · **Unblocks:** [GE-17](ge-17-windows-run-c.md) ·
**Branch:** `ge/16-windows-run-b` off `feature/git-epochs` for the plan notes and any missing tests, merged by rebase and
fast-forward. The tooling lives on the temporary `windows/git-epochs` branch and never lands. The only push in this plan
is that tooling branch, after Gordon says go.

## Goal

Run the epic\'s new TypeScript and extension tests on a real Windows machine, once, and write down what the run proves.
macOS and Linux cannot show how `git.exe`, `\r\n` checkouts, NTFS path forms, process trees, and Node file handles
behave for the code in `ts-packages/quarto-git-epochs` and `vscode-sync-experiment/`. This plan runs only the new
tests, not the workspace. It changes no product code: a defect becomes a proposed fix plan, which the parent inserts
after this plan (the fix plans cannot be listed in advance).

## Decisions implemented

- **G31:** three Windows runs from a temporary tooling branch that never lands, modeled on RFU-6, with the `windows/`
  prefix on the remote branch; each run covers only the new tests. This is run B.
- **G29:** the shared fixtures and scenario step lists (JSON, so Windows can run them) run natively here.
- **G30:** the vscode-free extension core and the headless peer run on Windows.
- **G22:** the Join command\'s `q2` spawn: ticket in memory only and never logged, a free port the extension picks, a
  readiness poll of up to 15 s, kill of the spawned process on Leave.
- **G24:** origination publishes offline first and resumes from local storage after an interruption.
- **G23, G4:** nested `.quarto-hub` and `.git` segments are ignored; git is called with plumbing only, and Windows
  path forms (`C:/…` from `--git-path`) are parsed.
- **G32:** the 15 s proxy readiness timeout and the 10 MB publish cap are the numbers under test.

## Context

- **The model** is workspace-9\'s branch `rfu/6-windows-tooling`: a PowerShell script of independent items that writes
  `results/<run>.json` and a `.md` summary, a failure extractor run with `uv`, and an instructions page. Its
  `Invoke-Logged` runs each command through `cmd.exe` and waits on that one process, because leaked grandchildren hold
  a PowerShell stdout pipe open for hours. Reuse GE-15\'s copies of these helpers; do not fork them.
- **Lessons from RFU-6.** Its two runs used a base 175 commits behind the integration tip, so neither exercised the
  finished work: merge the integration tip into the tooling branch first (never rebase a pushed branch) and record the
  base commit in the results. Its machine had
  `core.autocrlf=input`, so the CRLF case was not exercised: clone with `core.autocrlf=true`, and have the script
  report any other value as a caveat.
- **What GE-15 left on the tooling branch:** `scripts/git-epochs-windows/run.ps1`, the failure extractor, the results
  directory, the large-tree generator, and the instructions page. Read GE-15\'s notes for the exact names before
  extending them.
- **The code under test:** the TypeScript package (GE-5), the extension core and headless peer (GE-8), the follower
  modes (GE-9), clone mode (GE-10), Join (GE-11), and origination (GE-12).
- **Known Windows hazards for this code:** a read while git holds a file can fail with a sharing violation (a skip,
  never a delete); registry keys must stay `/` while `path.sep` is `\`; `Foo.qmd` and `foo.qmd` are one file;
  a directory rename fails while any handle is open inside it (`cross-platform.md`).

## Test specifications

Each item is independent: a failure never stops the others. Ids continue from GE-15\'s `a` items.

| Item | How it is tested | A pass proves | It does not prove |
|---|---|---|---|
| `b1` (W6) | `npm test -w @quarto/quarto-git-epochs` with `git.exe` on `PATH`: the probe, the controller against real markers, the shared scenario step lists, on temp repos in `%TEMP%`; jj scenarios skip when `jj` is absent | The probe parses Git for Windows output (`C:/…` from `--git-path`, `ls-tree -z` quoting, reflog); the gate sees real `index.lock` and `rebase-merge` | Other Git builds; jj on Windows unless it is installed |
| `b2` (W4, TypeScript) | The key and path tests: `/` keys, case-insensitive collisions, drive letters and UNC prefixes, nested `.quarto-hub` and `.git` segments, the `safeJoin` NTFS forms (`.git.`, trailing space, `.git::$INDEX_ALLOCATION`, an 8.3 name such as `GIT~1` where the volume has short names), a directory junction (needs no privilege) pointing outside the root, a path over 260 characters with `core.longpaths=true` | The TypeScript key normalization, ignore rules, and `safeJoin` hold for Windows path forms | Filesystems other than NTFS; Dev Drive or OneDrive behavior; volumes with 8.3 names disabled (the item then skips that row) |
| `b3` (W3, TypeScript) | The `\r\n` fixtures through the content rule and arrival, in a clone made with `core.autocrlf=true` | A CRLF checkout does not look dirty, and comparisons normalize as the macOS tests assume | `.gitattributes` filters (LFS, ident); `autocrlf=input` |
| `b4` | The vitest suites of the extension\'s vscode-free core and the headless peer, including GE-10\'s rebind (the Automerge store is deleted after the repo is shut down: no EBUSY) and the atomic write of `git-epochs.json` (a rename that fails with EPERM is retried) | The engine runs on Windows: the injected watcher, writes to disk, no handle held across a directory rename | VS Code\'s own file watcher |
| `b5` (W7) | The Join spawn module against a stub `q2.cmd` (spawned through a shell, as GE-11 specifies for `.cmd`; Node 20.12 and later refuses it otherwise): the ticket in argv, a free port, the readiness poll of the stub\'s `/health`, stdin close ending the stub, kill of the whole process tree on Leave, stderr surfaced, no ticket in any log. Optional: a real `q2 preview --share` host and `--join` guest on this machine if the network allows | Spawn and kill semantics on Windows; the ticket stays out of logs | The iroh tunnel across real networks |
| `b6` | Origination against a local hub: a headless originator publishes a generated repo to a local `q2 hub`, the publish is killed halfway and resumed from local storage, a second headless peer joins and reads every file; the share link appears only after the heads are confirmed | Publish order and resume on NTFS paths and Node file handles | The public server; very large uploads |
| `b7` | A clean root install: `npm ci`, then `npm run typecheck -w quarto-hub-vscode` and `npm run build -w quarto-hub-vscode` plus the `ts-packages` builds (`npm run build --if-present -w ts-packages/...`), from the repo root at the pinned Node. The root `npm run build` is not used: it also runs the hub-client and preview-SPA builds, which need the WASM and a network fetch of pandoc | The extension joined the workspaces cleanly and its bundle builds on Windows | `vsce package`; installing a `.vsix`; the hub-client and SPA builds |

Known failures to ignore: none at the start. Copy any from GE-15\'s notes that apply to this code.

## Checklist

Items marked (agent) run on macOS; items marked (Gordon) need the Windows machine.

- [ ] (agent) Confirm GE-11, GE-12, and GE-15 are complete, and read GE-15\'s notes for the tooling file names.
- [ ] (agent) Add any missing test for `b2` or `b3` on `ge/16-windows-run-b` first (tests only, no product code; they
  also run on macOS), land it by rebase and fast-forward, and note the commit.
- [ ] (agent) Bring the tooling branch up to the current tip by merging, never by rebasing a pushed branch:
  `git merge feature/git-epochs` run inside the tooling worktree GE-15 created,
  `/Users/gordon/src/q2/.worktrees/windows-git-epochs` (the branch is checked out there, so no lane worktree can switch
  to it; do all tooling work in that worktree, and keep this plan\'s own notes on `ge/16-windows-run-b` in your lane
  worktree). Record the base commit hash in the notes.
- [ ] (agent) Extend `scripts/git-epochs-windows/run.ps1` with items `b1` to `b7`. Each uses `Invoke-Logged` and
  `Add-Result`, fills `proves` and `does_not_prove`, and runs alone with `-Only`.
- [ ] (agent) Extend the instructions page on the tooling branch, `claude-notes/plans/git-epochs/windows-run-instructions.md`
  (detail chosen by the plan author; confirm it is the path GE-15 uses): Node at the `.nvmrc` major, a clean clone with
  `core.autocrlf=true` and `core.longpaths=true`, optional `jj`, `-SkipBuild` when GE-15\'s build is still in `target\`,
  and the table above.
- [ ] (agent) Gate the tooling before handoff: syntax-check the script with `pwsh` if installed (parse only, run
  nothing); `uv run scripts/git-epochs-windows/extract_failures.py --help`; `q2 render --strict` the instructions page.
- [ ] (agent) Ask Gordon to say go. Then push the tooling branch with the explicit refspec, unsilenced:
  `git push origin windows/git-epochs:windows/git-epochs`, and check that `git ls-remote origin windows/git-epochs`
  prints the output of `git rev-parse HEAD`.
- [ ] (Gordon) On the Windows machine: `git fetch origin windows/git-epochs`, switch to it, and run
  `powershell -ExecutionPolicy Bypass -File scripts\git-epochs-windows\run.ps1 -Only b1,b2,b3,b4,b5,b6,b7 -Publish`.
  If a failure needs explaining, run the extractor once and push `failure-details`.
- [ ] (agent) Fetch the branch, parse `results/*.json`, and record the run in the notes: run id, host, Windows version,
  git, node, uv, the `core.autocrlf` value, the base commit, a table of item results, and every failure with its output.
- [ ] (agent) For each failure decide whether it is a product defect, a test defect, or the machine, and write a
  proposed fix plan (title, cause, files, test) in the notes. Do not change product code in this plan.
- [ ] (agent) Copy the summary into the notes on `ge/16-windows-run-b`; rebase onto `feature/git-epochs`,
  fast-forward, and leave the tooling branch unmerged.

## Gates

- The tooling gates in the checklist (script parse, extractor help, instructions page render).
- If a test was added: `npm test -w @quarto/quarto-git-epochs` and `npm run typecheck` on macOS.
- No Rust gates: this plan changes no Rust code.

## End-to-end verification

The Windows run is the verification. The macOS side checks only that the results file parses and that every item has
a recorded outcome. The agent cannot observe Windows, so the notes say plainly which items ran, which were read, and
which were not run.

## Exit criteria

- Every item `b1` to `b7` has a result (ran on Windows, read, or not run) in the notes, with failures listed and their
  output attached.
- Each failure has a classification and a proposed fix plan.
- No product code changed; the tooling branch is unmerged and still on the remote for GE-17.

## Out of scope

- Fixing anything the run finds (fix plans follow this plan).
- The Rust and hub items (GE-15), and the scripted scenario and the Playwright subset (GE-17).
- Failures in older tests that predate the epic; list them, do not chase them.
- jj on Windows beyond skipping cleanly when it is absent.

## Notes from the run

Not started.
