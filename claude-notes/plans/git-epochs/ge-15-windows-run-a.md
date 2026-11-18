---
title: 'GE-15: Windows run A: the Rust and hub items'
date: 2026-11-18
description: 'Runs only the new Rust and hub tests of the git epochs epic on a real Windows machine, from a temporary tooling branch that never lands, with specific items and a record of what each one proves.'
status: approved
depends-on: [ge-4]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-4](ge-04-hub-surface.md) ·
**Unblocks:** [GE-16](ge-16-windows-run-b.md), [GE-17](ge-17-windows-run-c.md) ·
**Branches:** the plan\'s notes land through `ge/15-windows-run-a` off `feature/git-epochs` (rebase and fast-forward).
The tooling lives on a separate temporary branch, `windows/git-epochs`, that never lands. This plan is the epic\'s one
exception to "no pushes": it pushes that branch, only after Gordon says go.

## Goal

Run the epic\'s new Rust code on Windows, where it cannot be tested from macOS: the git probe, the gate, the
classifier and arrival comparisons under `core.autocrlf=true`, the path and key handling, and the hub gate with a real
repo. Windows is not in CI, and `cargo clippy --target x86_64-pc-windows-msvc` type-checks but cannot link
(the `aws-lc` build scripts), so only a Windows run shows locking, line-ending, and path behavior
(`.claude/rules/cross-platform.md`). The run covers only the new tests, and every item says what a pass proves and what
it does not.

## Why it waits

It runs once the Rust side is complete: the crate ([GE-2](ge-02-rust-logic-probe-controller.md)), the hub integration
([GE-3](ge-03-hub-integration.md)), and the surface (GE-4). Running earlier would test code that is still changing.

## Decisions implemented

- **G31.** Windows is tested from a temporary tooling branch that never lands, modeled on RFU-6, with the `windows/`
  prefix on the remote branch; each run covers only the new tests.
- **G3, G4, G5, G12, G6** are what the items exercise: the markers and probe, the gate in `sync_all`, the
  case-insensitive key collision rule, and the registry keys.

## Context

- **The model** is workspace-9\'s branch `rfu/6-windows-tooling` (read-only): `scripts/rfu6-windows/run.ps1`,
  `scripts/rfu6-windows/extract_failures.py`, and `claude-notes/plans/rfu-6-windows-run-instructions.md`, with the
  plan `2026-11-08-rfu-6-windows-run.md`. The tooling commits there are labeled "temporary, never lands"; results
  come back as committed JSON and markdown; logs are not committed.
- **What to keep from `run.ps1`:** PowerShell 5.1 compatibility; independent items (a failure never stops the
  rest); `Invoke-Logged`, which runs a command through `cmd.exe` and waits on that one process, because a leaked
  grandchild can hold a pipe open for hours; `Invoke-Nextest` with an `-E` filter and a `MinTests` guard so a filter
  that matches too little fails; `Add-Result` with `proves` and `does_not_prove` fields; `-Only`, `-SkipBuild`,
  `-WithTools` (downloads the pandoc and typst versions CI pins into `results/tools` and puts them on `PATH`),
  `-Publish`; the JSON and markdown summary; a `.gitignore` for `logs/`, `venv/`, and `tools/`.
- **Lessons from the RFU-6 runs:** the first two runs used a stale base, so record the base commit and check it is
  the current tip before a run; the machine had `core.autocrlf=input`, so the CRLF case was never exercised, so the
  script must read `core.autocrlf` and mark the CRLF items with a caveat when it is not `true`; the RFU-6 run found
  real defects (a `\\?\` verbatim path from `canonicalize`), which is the kind of finding to expect.
- **Filter form.** Integration tests are one binary per crate, so a filter reads
  `package(quarto-hub) & binary(integration) & test(<file>::)` (`.claude/rules/integration-tests.md`).
- **Module names are placeholders** below: GE-2 and GE-3 fix the real names. Update the filters before writing the
  script, and run each filter once on macOS (`cargo nextest list -E "<filter>"`) so it matches what it should.

## The items

Script item ids are named by run: `a1` to `a6` here, `b1` to `b7` in [GE-16](ge-16-windows-run-b.md), `c1` to `c3` in
[GE-17](ge-17-windows-run-c.md). Each item carries the catalog number (W1 to W10) of the epic's Windows list in parentheses.
The catalog is shared across the runs: this run holds W1 to W5 and W10, run B holds W6 and W7 and the TypeScript halves
of W3 and W4, run C holds W8 and W9.

- **`a1` (W1), the git probe on Git for Windows.** Filter: `package(quarto-git-epochs) & test(probe::)`.
  - Proves: `rev-parse --git-path` output with a drive letter (`C:/...`) and from a linked worktree parses; `ls-tree -r
    -z` with a project prefix returns the right paths; symlink entries (mode 120000) are filtered; `cat-file --batch`
    frames binary content correctly; reflog subjects parse.
  - Does not prove: any Git build other than Git for Windows, or colocated jj.
- **`a2` (W2), the gate during a real `git switch`.** Generate a deterministic tree of 4,000 files with
  `uv run scripts/git-epochs-windows/make_large_tree.py`, then switch between two branches with real git while the
  controller test runs. Filter: `package(quarto-git-epochs) & binary(integration) & test(large_tree::)`.
  - Proves: no sync or merge happens while a marker exists; a sharing-violation or access-denied read error is a skip,
    never a delete.
  - Does not prove: antivirus or OneDrive interplay, or timings on a cold cache.
- **`a3` (W3), `core.autocrlf=true`.** Filter: `package(quarto-git-epochs) & (test(classify::) | test(arrival::))`, run in a
  checkout where the script confirms `core.autocrlf` is `true`.
  - Proves: a CRLF-checked-out file compares equal to its blob content after normalization, through the classifier
    and the arrival comparison, for text; binaries compare by bytes.
  - Does not prove: `.gitattributes` filters (LFS, `ident`), or the CRLF case if `autocrlf` was not `true` (the
    script marks that caveat).
- **`a4` (W4), paths and keys.** Filters: `package(quarto-git-epochs) & test(registry::)` and
  `package(quarto-hub) & test(path_keys::)`.
  - Proves: registry keys use `/`; a case-insensitive collision of two keys falls back to ordinary sync; drive letters,
    UNC prefixes, and long paths (with `core.longpaths`) round-trip.
  - Does not prove: NTFS 8.3 short names or Unicode normalization differences.
- **`a5` (W5), hub gate and checkpoint integration.** Filter:
  `package(quarto-hub) & binary(integration) & test(git_epochs::)`, with a real temp repo under the Windows temp
  directory.
  - Proves: the gate inside `sync_all` holds with a real repo; the checkpoint is written before the pointer flips; no
    code path renames a directory while it holds a handle in it (`cross-platform.md`).
  - Does not prove: network filesystems or Dev Drive behavior.
- **`a6` (W10), native clippy and nextest.** `cargo clippy -p quarto-git-epochs -p quarto-hub -p quarto-preview -p quarto
  --all-targets -- -D warnings`, then `cargo nextest run` for the same four crates.
  - Proves: the epic\'s own gate is green on the Windows host target.
  - Does not prove: pandoc and typst dependent tests unless `-WithTools` was given. Failures that appear on the
    macOS baseline are listed as known unrelated.

## Test specifications

The tooling is code, so it is tested before it ships, on macOS, in this order:

1. `make_large_tree.py`: run twice with one seed; the file count and a hash of all contents are identical; the repo
   it creates has two branches whose trees differ in a known number of files.
2. `extract_failures.py`: a captured nextest log with known failures (a fixture committed on the tooling branch)
   yields the expected grouping by first panic message and the per-module counts.
3. `run.ps1` parses (`pwsh -NoProfile` loads it as a script block, if `pwsh` is available; otherwise note that the
   syntax was reviewed only), and each item\'s filter lists at least its `MinTests` tests on macOS.

## Checklist

- [ ] Confirm GE-2, GE-3, and GE-4 are complete; read their notes for the real module and test names. Replace the
  placeholders above and record the final filters in "Notes from the run".
- [ ] Write the tooling tests first (items 1 to 3 above) and watch them fail.
- [ ] Create the tooling branch and its worktree: `git worktree add -b windows/git-epochs
  .worktrees/windows-git-epochs feature/git-epochs`. Commit the tooling in small commits, each message ending
  "(temporary, never lands)".
- [ ] Write `scripts/git-epochs-windows/run.ps1`, adapted from RFU-6 (not copied blindly): items `a1` to `a6`,
  a build item, the preflight that prints the install command for anything missing, `-WithTools`, the
  `core.autocrlf` check and caveat, the base-commit check, JSON and markdown results.
- [ ] Write `scripts/git-epochs-windows/extract_failures.py` (standard library only, run with `uv run`),
  `scripts/git-epochs-windows/make_large_tree.py`, and the results `.gitignore`.
- [ ] Write the instructions page `claude-notes/plans/git-epochs/windows-run-instructions.md`, on the tooling branch
  only: one-time setup (git, Rust MSVC, `cargo-nextest`, `git config --global core.longpaths true`, Node at `.nvmrc`,
  uv, the Visual Studio C++ build tools); cloning with `-c core.autocrlf=true`; the run command; sending results
  back; a table of what each item proves and does not prove; known failures to ignore; how to explain failures; and
  how to refresh the branch (merge `feature/git-epochs` into it; never rebase a pushed branch). Render-check it with
  `q2 render --strict`.
- [ ] Run every item\'s command on macOS where it applies and list the failures there as known unrelated. Estimate the
  Windows cost (a cold build of four crates and their test binaries is plausibly 15 to 30 minutes and 10 to 20 GB;
  an estimate, not measured).
- [ ] **Gordon:** say go. The agent pushes with an explicit refspec,
  `git push origin windows/git-epochs:windows/git-epochs`, never silenced, and confirms the remote tip with
  `git ls-remote origin windows/git-epochs` equal to `git rev-parse windows/git-epochs`.
- [ ] **Gordon:** on the Windows machine, clone with `core.autocrlf=true`, switch to the branch, and run
  `powershell -ExecutionPolicy Bypass -File scripts\git-epochs-windows\run.ps1 -WithTools -Publish`.
- [ ] **Gordon:** tell the agent "results are pushed".
- [ ] The agent fetches the branch, parses the JSON, and records each item as PASS, FAIL, or SKIP in the notes with
  the base commit, the machine description, and the `core.autocrlf` value. If anything failed, run the extractor
  and add its summary.
- [ ] List every finding with its output. Do not change product code in this plan: propose a fix plan for each
  defect, to be inserted after this plan.
- [ ] Land the notes on `feature/git-epochs` through `ge/15-windows-run-a`; the tooling is not merged. Verify with
  `git log feature/git-epochs -- scripts/git-epochs-windows`, which must be empty.

## Gates

Before the push, the macOS baseline of every item\'s command passes or has its failures listed as known unrelated:

```bash
cargo nextest run -p quarto-git-epochs -p quarto-hub -p quarto-preview -p quarto
cargo clippy -p quarto-git-epochs -p quarto-hub -p quarto-preview -p quarto --all-targets -- -D warnings
```

## End-to-end verification

The run itself, on the Windows machine, is the verification. The recorded results file is the evidence: its path and
the items it reports go in the notes, and the notes say the output was read.

## Exit criteria

- Every item has a recorded result (ran on Windows with the base commit, machine, and `autocrlf` value, or not run
  with a reason); failures are listed with their output; findings have proposed fix plans.
- No product code changed; the tooling never reached `feature/git-epochs`.

## Out of scope

- Fixing what the run finds (inserted fix plans), the TypeScript and extension items (GE-16), and the scripted
  scenario and Playwright subset (GE-17).
- Adding Windows to CI.

## Notes from the run

Not started.
